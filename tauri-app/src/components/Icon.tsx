import {
  ActivityLogIcon,
  ArrowDownIcon,
  ArrowTopRightIcon,
  ArrowUpIcon,
  CheckIcon,
  Cross1Icon,
  DashboardIcon,
  DownloadIcon,
  ExternalLinkIcon,
  FileIcon,
  GearIcon,
  LightningBoltIcon,
  MagicWandIcon,
  Pencil1Icon,
  PlayIcon,
  PlusIcon,
  ReloadIcon,
  ResetIcon,
  RocketIcon,
  StopIcon,
  TableIcon,
  TrashIcon,
  UpdateIcon,
  UploadIcon,
} from "@radix-ui/react-icons";

type RadixIcon = typeof ActivityLogIcon;

const ICONS = {
  active: LightningBoltIcon,
  add: PlusIcon,
  check: CheckIcon,
  close: Cross1Icon,
  dashboard: DashboardIcon,
  down: ArrowDownIcon,
  edit: Pencil1Icon,
  export: DownloadIcon,
  file: FileIcon,
  import: UploadIcon,
  launch: RocketIcon,
  logs: ActivityLogIcon,
  models: TableIcon,
  patch: MagicWandIcon,
  play: PlayIcon,
  refresh: ReloadIcon,
  reset: ResetIcon,
  restore: ResetIcon,
  save: CheckIcon,
  settings: GearIcon,
  stop: StopIcon,
  trash: TrashIcon,
  up: ArrowUpIcon,
  update: UpdateIcon,
  external: ExternalLinkIcon,
  open: ArrowTopRightIcon,
} as const satisfies Record<string, RadixIcon>;

export type IconName = keyof typeof ICONS;

export default function Icon({ name }: { name: IconName }) {
  const Component = ICONS[name];
  return (
    <span className="icon" aria-hidden="true">
      <Component />
    </span>
  );
}
