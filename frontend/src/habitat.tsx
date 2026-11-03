export function Habitat() {
  return (
    <svg className="habitat" viewBox="0 0 1200 380" aria-hidden="true" preserveAspectRatio="xMidYMid slice">
      <path className="cloud cloud-one" d="M86 104c17-32 59-29 72 0 26-12 56 7 52 36H45c-7-28 19-50 41-36Z" />
      <path className="cloud cloud-two" d="M910 76c12-23 44-23 56 0 20-8 43 7 39 29H875c-5-21 15-37 35-29Z" />
      <path className="migration" d="M56 171C263 60 393 85 536 151c183 84 336 59 602-56" />
      <g className="tern" transform="translate(428 114)">
        <path d="M0 18c20-2 35-8 50-18-2 13-9 24-22 32 18-4 33-2 47 8-23 3-43 0-59-9C3 38-11 40-27 37-14 31-5 25 0 18Z" />
        <path className="wing" d="M14 19C7 2 0-9-9-17c22 4 38 13 47 27-7 7-15 11-24 9Z" />
        <circle cx="50" cy="17" r="2.5" />
      </g>
      <path className="water far" d="M0 220c92-28 174 28 264 0s173 28 263 0 174 28 264 0 176 28 409 0v160H0Z" />
      <path className="water near" d="M0 258c105-31 200 31 305 0s198 31 303 0 200 31 305 0 200 31 287 5v117H0Z" />
      <g className="fish fish-one" transform="translate(760 294)">
        <path d="M0 0c24-22 67-20 88 0-21 23-64 23-88 0Zm86 0 30-22v44Z" />
        <circle cx="18" cy="-4" r="3" />
      </g>
      <g className="fish fish-two" transform="translate(240 330) scale(.7)">
        <path d="M0 0c24-22 67-20 88 0-21 23-64 23-88 0Zm86 0 30-22v44Z" />
        <circle cx="18" cy="-4" r="3" />
      </g>
      <g className="reeds" transform="translate(1050 320)">
        <path d="M0 60C0 19 5-3 17-35M12 60c2-37 15-55 33-75M28 60c-2-28 3-48 17-61" />
        <ellipse cx="17" cy="-34" rx="5" ry="13" />
      </g>
    </svg>
  );
}

export function Mark() {
  return (
    <svg viewBox="0 0 64 64" aria-hidden="true">
      <path d="M8 27c9-9 19-10 29-3 6 4 12 4 19 0-2 11-10 18-22 18-11 0-20-5-26-15Z" />
      <path d="M20 25C17 14 11 9 5 5c12 0 22 5 29 15-4 4-9 6-14 5Z" />
      <path d="M23 49c8-7 20-7 27 0-7 8-19 8-27 0Zm26 0 9-7v14Z" />
    </svg>
  );
}
