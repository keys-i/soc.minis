export function Footer() {
  return (
    <footer className="tectonic-footer">
      <div className="tectonic-field" aria-hidden="true">
        <span className="tectonic-plate plate-west" />
        <span className="tectonic-plate plate-east" />
        <span className="tectonic-plate plate-ridge" />
      </div>
      <a className="footer-wordmark" href="https://keysi.dev">
        keysi.dev
      </a>
      <nav aria-label="Footer">
        <a
          className="footer-symbol footer-github"
          href="https://github.com/keys-i"
          target="_blank"
          rel="noopener noreferrer"
          aria-label="GitHub"
          title="GitHub"
        />
        <a
          className="footer-symbol footer-linkedin"
          href="https://linkedin.com/in/radhesh-g"
          target="_blank"
          rel="noopener noreferrer"
          aria-label="LinkedIn"
          title="LinkedIn"
        />
      </nav>
    </footer>
  );
}
