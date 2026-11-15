<script lang="ts">
  import {
    House, Library, CircleArrowUp, Download, Settings2, Plus, Search,
    ArrowUpRight, ArrowRight, ChevronRight, ChevronDown, Check, X, LoaderCircle,
    RefreshCw, CircleHelp, ExternalLink, FolderOpen, Sun, Moon, Monitor,
    ShieldCheck, Shield, CircleAlert, CircleCheck, Info, FileDown, FileUp, Copy,
    Eye, EyeOff, Trash2, ArrowDownToLine, Package, HardDrive, Ban, Clock3,
    SlidersHorizontal, FileText, Code2, ListFilter, Wifi, WifiOff, RotateCcw,
    Star, GitFork, Activity, UserRound, LogOut, BookOpen, Pencil, LockKeyhole,
  } from '@lucide/svelte';

  const icons = {
    home: House, library: Library, updates: CircleArrowUp, download: Download,
    settings: Settings2, plus: Plus, search: Search, arrowUpRight: ArrowUpRight,
    arrowRight: ArrowRight, chevronRight: ChevronRight, chevronDown: ChevronDown,
    check: Check, close: X, loader: LoaderCircle, refresh: RefreshCw, github: Code2,
    help: CircleHelp, external: ExternalLink, folder: FolderOpen, sun: Sun, moon: Moon,
    monitor: Monitor, shieldCheck: ShieldCheck, shield: Shield, alert: CircleAlert,
    success: CircleCheck, info: Info, export: FileDown, import: FileUp, copy: Copy,
    eye: Eye, eyeOff: EyeOff, trash: Trash2, install: ArrowDownToLine, package: Package,
    drive: HardDrive, cancel: Ban, clock: Clock3, sliders: SlidersHorizontal,
    notes: FileText, code: Code2, filter: ListFilter, connected: Wifi, disconnected: WifiOff,
    retry: RotateCcw, star: Star, fork: GitFork, activity: Activity, user: UserRound,
    logout: LogOut, readme: BookOpen, edit: Pencil, private: LockKeyhole,
  };
  let { name, size = 18, class: className = '', strokeWidth = 1.8 }: {
    name: keyof typeof icons; size?: number; class?: string; strokeWidth?: number;
  } = $props();
  const Glyph = $derived(icons[name]);
</script>

<Glyph {size} {strokeWidth} class={className} aria-hidden="true" />
