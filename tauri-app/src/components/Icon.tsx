const ICONS = {
  active: "●",
  add: "+",
  check: "✓",
  close: "×",
  dashboard: "◎",
  down: "↓",
  edit: "✎",
  file: "▤",
  launch: "↗",
  logs: "≡",
  models: "▦",
  patch: "!",
  play: "▶",
  refresh: "↻",
  reset: "↺",
  restore: "↶",
  save: "✓",
  settings: "⚙",
  stop: "■",
  trash: "×",
  up: "↑",
} as const;

export type IconName = keyof typeof ICONS;

export default function Icon({ name }: { name: IconName }) {
  return (
    <span className="icon" aria-hidden="true">
      {ICONS[name]}
    </span>
  );
}
