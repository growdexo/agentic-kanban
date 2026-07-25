export function Logo() {
  return (
    <svg
      width="150"
      viewBox="0 0 384 72"
      xmlns="http://www.w3.org/2000/svg"
      className="logo"
      role="img"
      aria-label="Agentic Kanban"
    >
      <rect x="4" y="18" width="11" height="38" rx="3" fill="#E9823C" />
      <rect x="20" y="10" width="11" height="46" rx="3" fill="#E9823C" opacity="0.7" />
      <rect x="36" y="26" width="11" height="30" rx="3" fill="#E9823C" opacity="0.45" />
      <text
        x="60"
        y="47"
        fontFamily="'IBM Plex Sans', system-ui, -apple-system, Segoe UI, Roboto, sans-serif"
        fontSize="30"
        fontWeight="700"
        letterSpacing="-0.5"
        fill="currentColor"
      >
        Agentic <tspan fill="#E9823C">Kanban</tspan>
      </text>
    </svg>
  );
}
