const ICONS = {
  active: "●",
  add: "+",
  check: "✓",
  close: "×",
  dashboard: "◎",
  down: "↓",
  edit: "✎",
  export: "⇩",
  file: "▤",
  import: "⇧",
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
  update: "⬆",
} as const;

export type IconName = keyof typeof ICONS;

export default function Icon({ name }: { name: IconName }) {
  return (
    <span className="icon" aria-hidden="true">
      {ICONS[name]}
    </span>
  );
}
