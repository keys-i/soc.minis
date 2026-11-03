use std::{env, path::PathBuf, time::Duration};

use clap::{Args, Parser, Subcommand};
use soc_minis::{
    agent::{self, Watch},
    cache::ArtifactCache,
    db::Store,
    error::{Result, invalid},
    event::{self, Options},
    forms::{self, FormProfile},
    graph::{self, DeviceStatus},
    model::{Event, OutputMode, RiskLayout},
    rooms, web,
};

const CACHE_BUDGET: u64 = 128 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "soc",
    version,
    about = "Prepare and follow student society events"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Prepare a Forms preview and editable risk documents
    Event(EventArgs),
    /// Search the UQ room catalogue
    Rooms(RoomArgs),
    /// Connect Outlook and follow booking replies
    Agent(AgentArgs),
    /// Run the SOC Minis website and background watcher
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
struct EventArgs {
    /// Editable event TOML
    source: PathBuf,
    /// Generate only this output, otherwise use the event file
    #[arg(long, value_enum)]
    mode: Option<OutputMode>,
    /// Generate the risk form, pack, or both
    #[arg(long, value_enum)]
    layout: Option<RiskLayout>,
    /// Directory for generated files
    #[arg(short, long, default_value = "out/event")]
    output_dir: PathBuf,
    /// Replace existing generated files
    #[arg(long)]
    force: bool,
    /// Submit the prepared booking after writing its Forms preview
    #[arg(long)]
    submit: bool,
}

#[derive(Debug, Args)]
struct RoomArgs {
    /// Building, room, campus, kind, or capacity phrase
    query: String,
    /// Minimum seats
    #[arg(short, long, default_value_t = 0)]
    minimum: u32,
    /// Optional replacement CSV or CSV.GZ catalogue
    #[arg(long)]
    catalogue: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct AgentArgs {
    #[command(subcommand)]
    command: AgentCommand,
    /// Private SOC state directory
    #[arg(long, env = "SOC_DATA", default_value = ".config/soc", global = true)]
    data: PathBuf,
    /// Owner identity for local terminal state
    #[arg(long, env = "SOC_OWNER", default_value = "local", global = true)]
    owner: String,
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    /// Connect the registered Microsoft 365 mailbox
    Login {
        #[arg(long)]
        mailbox: String,
        #[arg(long, env = "SOC_MICROSOFT_CLIENT_ID")]
        client_id: String,
        #[arg(long, env = "SOC_MICROSOFT_TENANT_ID")]
        tenant_id: String,
    },
    /// Follow one booking and authorise its risk-assessment return
    Watch {
        source: PathBuf,
        #[arg(long)]
        event_id: String,
        #[arg(long)]
        attachment: Option<PathBuf>,
        #[arg(long, default_value = "pfeventa@uq.edu.au")]
        recipient: String,
        #[arg(long)]
        since: Option<String>,
    },
    /// Process one mailbox pass
    Poll,
    /// Show saved booking progress
    Status,
    /// Permit a retry after checking Outlook Sent Items
    RetrySend {
        event_id: String,
        #[arg(long)]
        confirm_not_sent: bool,
    },
    /// Stop following an event while preserving its history
    Stop { event_id: String },
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[arg(long, env = "SOC_BIND", default_value = "127.0.0.1:8787")]
    bind: String,
    #[arg(long, env = "SOC_BASE", default_value = "/soc")]
    base: String,
    #[arg(long, env = "SOC_DATA", default_value = ".config/soc")]
    data: PathBuf,
    #[arg(long, env = "SOC_FRONTEND", default_value = "frontend/dist")]
    frontend: PathBuf,
    #[arg(long, env = "SOC_POLL_SECONDS", default_value_t = 60)]
    poll_seconds: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "soc_minis=info,tower_http=info".into()),
        )
        .init();
    match Cli::parse().command {
        Command::Event(arguments) => prepare(arguments).await,
        Command::Rooms(arguments) => search_rooms(&arguments),
        Command::Agent(arguments) => run_agent(arguments).await,
        Command::Serve(arguments) => {
            web::serve(web::Config {
                bind: arguments.bind,
                base: arguments.base,
                data: arguments.data,
                frontend: arguments.frontend,
                poll_interval: Duration::from_secs(arguments.poll_seconds.max(15)),
            })
            .await
        }
    }
}

async fn prepare(arguments: EventArgs) -> Result<()> {
    let configured = Event::load(&arguments.source)?;
    let profile = FormProfile::bundled()?;
    let cache = ArtifactCache::new(data_root().join("cache"), CACHE_BUDGET)?;
    let generated = event::generate(
        &arguments.source,
        &Options {
            mode: arguments.mode.unwrap_or(configured.output_mode),
            layout: arguments.layout.unwrap_or(configured.risk_layout),
            destination: arguments.output_dir,
            force: arguments.force,
        },
        &cache,
        &profile,
    )?;
    for output in &generated.outputs {
        println!("{}\t{}", output.kind, output.path.display());
    }
    if arguments.submit {
        let payload = generated.payload.ok_or_else(|| {
            invalid("Choose form or both output before requesting a Forms submission")
        })?;
        let status = forms::submit(&profile, &payload, &profile.secrets()?).await?;
        println!("submitted\tMicrosoft Forms HTTP {}", status.as_u16());
    }
    Ok(())
}

fn search_rooms(arguments: &RoomArgs) -> Result<()> {
    let catalogue = rooms::read(arguments.catalogue.as_deref())?;
    for room in rooms::search(&catalogue, &arguments.query, arguments.minimum)? {
        println!(
            "{}\t{}\t{}\t{} seats\t{}",
            room.campus, room.room, room.building, room.seats, room.kind
        );
    }
    Ok(())
}

async fn run_agent(arguments: AgentArgs) -> Result<()> {
    let store = Store::open(&arguments.data)?;
    let cache = ArtifactCache::new(arguments.data.join("cache"), CACHE_BUDGET)?;
    match arguments.command {
        AgentCommand::Login {
            mailbox,
            client_id,
            tenant_id,
        } => {
            let flow = graph::start_device_login(
                &store,
                &arguments.owner,
                &mailbox,
                &client_id,
                &tenant_id,
            )
            .await?;
            println!("Open {}", flow.verification_uri);
            println!("Enter {}", flow.user_code);
            loop {
                tokio::time::sleep(Duration::from_secs(flow.interval_seconds.max(1))).await;
                match graph::poll_device_login(&store, &arguments.owner, &flow.id).await? {
                    DeviceStatus::Pending { retry_after } => {
                        tokio::time::sleep(Duration::from_secs(
                            retry_after.saturating_sub(flow.interval_seconds),
                        ))
                        .await;
                    }
                    DeviceStatus::Connected { mailbox } => {
                        println!("Connected {mailbox}");
                        break;
                    }
                }
            }
        }
        AgentCommand::Watch {
            source,
            event_id,
            attachment,
            recipient,
            since,
        } => {
            let status = agent::watch(
                &store,
                &cache,
                &arguments.owner,
                &Watch {
                    source: &source,
                    event_id: &event_id,
                    attachment: attachment.as_deref(),
                    recipient: &recipient,
                    since: since.as_deref(),
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        AgentCommand::Poll => agent::poll(&store, &arguments.owner).await?,
        AgentCommand::Status => println!(
            "{}",
            serde_json::to_string_pretty(&agent::status(&store, &arguments.owner)?)?
        ),
        AgentCommand::RetrySend {
            event_id,
            confirm_not_sent,
        } => agent::retry_send(&store, &arguments.owner, &event_id, confirm_not_sent)?,
        AgentCommand::Stop { event_id } => agent::stop(&store, &arguments.owner, &event_id)?,
    }
    Ok(())
}

fn data_root() -> PathBuf {
    env::var_os("SOC_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".config/soc"))
}
