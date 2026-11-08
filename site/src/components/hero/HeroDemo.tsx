import { useEffect, useState } from 'react'
import {
  ArrowLeftRight,
  Clock,
  CornerDownLeft,
  FolderTree,
  KeyRound,
  Menu,
  MonitorCog,
  Pause,
  Play,
  Plus,
  ScrollText,
  Search,
  Server,
  Settings,
  ShieldCheck,
  StickyNote,
  X,
  type LucideIcon,
} from 'lucide-react'
import { Button, Caret, Collapse, Popover, Stage, Window, useReducedMotion, type Act } from '../../stageshow/kit'
import '../../stageshow/kit/stageshow.css'
import './hero.css'

// The hero: LiteTerm connecting to a host from the command palette.
//
// One gesture, one payoff. Ctrl+K (Cmd+K on a Mac) opens the palette, "prod"
// narrows the list, Enter opens a tab and the terminal comes up connected,
// where `uptime` answers. Closing the session with Ctrl+W is what returns the
// loop to the hosts list, so the reset has a cause the viewer can see.
//
// Layout: "Wide hero" from the stageshow framing table, a 960px window inset
// from the top and sides and cropped by the stage at the foot, on the page's
// own dark ground (no plate). On a phone the compact design drops the rail
// and keeps the topbar, the list, the palette and the terminal.
//
// Real strings: the hosts, what is typed, and what the server prints. The
// rail, the tabs and the buttons use the app's own labels and its own icon
// set (lucide), at one size and one stroke.

type S = {
  palette: boolean
  query: string
  view: 'hosts' | 'term'
  tab: boolean
  live: boolean
  motd: boolean
  prompt: boolean
  cmd: string
  out: boolean
}

const initial: S = {
  palette: false,
  query: '',
  view: 'hosts',
  tab: false,
  live: false,
  motd: false,
  prompt: false,
  cmd: '',
  out: false,
}

// The frame most people see: the payoff, with the result left on screen
const poster: S = {
  palette: false,
  query: '',
  view: 'term',
  tab: true,
  live: true,
  motd: true,
  prompt: true,
  cmd: 'uptime',
  out: true,
}

async function script(a: Act<S>) {
  const mac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent)
  await a.wait(1200)
  a.mark('palette')
  await a.key(mac ? 'cmd+k' : 'ctrl+k')
  await a.set({ palette: true })
  await a.wait(600)
  await a.type('query', 'prod', { interval: 160 })
  await a.wait(700)
  a.mark('connect')
  await a.key('enter')
  await a.set({ palette: false, tab: true, view: 'term' })
  await a.wait(800)
  await a.set({ motd: true, live: true })
  await a.wait(700)
  await a.set({ prompt: true })
  await a.wait(500)
  await a.type('cmd', 'uptime', { interval: 100 })
  await a.wait(350)
  await a.set({ out: true })
  a.mark('payoff')
  await a.wait(2800)
  // The way a person would end it: close the session, land back on the hosts
  await a.key(mac ? 'cmd+w' : 'ctrl+w')
  await a.set({ tab: false, view: 'hosts' })
  await a.wait(900)
  // Everything the loop added goes back while the terminal is out of sight
  await a.snap({ query: '', live: false, motd: false, prompt: false, cmd: '', out: false })
}

interface Host {
  label: string
  user: string
  address: string
}

const GROUPS: { name: string; hosts: Host[] }[] = [
  {
    name: 'Production',
    hosts: [
      { label: 'prod-api', user: 'ubuntu', address: '10.0.4.12' },
      { label: 'prod-db', user: 'postgres', address: '10.0.4.20' },
      { label: 'prod-worker', user: 'ubuntu', address: '10.0.4.31' },
    ],
  },
  {
    name: 'Staging',
    hosts: [
      { label: 'staging-web', user: 'deploy', address: '10.1.2.8' },
      { label: 'build-runner', user: 'ci', address: '10.0.9.3' },
    ],
  },
  {
    name: 'Other',
    hosts: [
      { label: 'bastion', user: 'ops', address: '203.0.113.7:2222' },
      { label: 'nas', user: 'admin', address: '192.168.1.50' },
    ],
  },
]

const HOSTS: Host[] = GROUPS.flatMap((g) => g.hosts)

const RAIL: { label: string; icon: LucideIcon }[] = [
  { label: 'Hosts', icon: MonitorCog },
  { label: 'Keychain', icon: KeyRound },
  { label: 'Port Forwarding', icon: ArrowLeftRight },
  { label: 'Snippets', icon: ScrollText },
  { label: 'Notes', icon: StickyNote },
  { label: 'Known Hosts', icon: ShieldCheck },
  { label: 'Logs', icon: Clock },
]

const ICON = { size: 16, strokeWidth: 1.5 } as const

const LABEL =
  'An illustration of LiteTerm: Ctrl+K opens quick connect, typing prod narrows the host list, and Enter opens a connected terminal on prod-api.'

// The loop runs for good, so it needs a way to stop it (WCAG 2.2.2). Paused,
// the stage shows its poster, the payoff frame; Play starts the story again.
// Under reduced motion the stage already shows only the poster, so there is
// nothing to pause and no button.
export default function HeroDemo() {
  const [playing, setPlaying] = useState(true)
  const reduced = useReducedMotion()
  // One control for all of the hero's motion: the backdrop (HeroBackdrop.astro) pauses on this attribute
  useEffect(() => {
    document.documentElement.toggleAttribute('data-hero-paused', !playing)
  }, [playing])
  return (
    <div className="lt-demo">
      <div role="img" aria-label={LABEL}>
        <Scene key={playing ? 'playing' : 'paused'} playing={playing} />
      </div>
      {!reduced && (
        <button type="button" className="lt-toggle" onClick={() => setPlaying((p) => !p)}>
          {playing ? <Pause size={14} strokeWidth={2} aria-hidden /> : <Play size={14} strokeWidth={2} aria-hidden />}
          {playing ? 'Pause animation' : 'Play animation'}
        </button>
      )}
    </div>
  )
}

function Scene({ playing }: { playing: boolean }) {
  return (
    <Stage<S>
      width={1040}
      height={520}
      compact={{ below: 640, width: 358, height: 340 }}
      theme="dark"
      fidelity="realistic"
      keys={{ wide: 'bottom-right', compact: 'bottom-left' }}
      initial={playing ? initial : poster}
      poster={poster}
      script={playing ? script : undefined}
      className="lt"
    >
      {(s, _rect, layout) => (
        <Window
          className="lt-win"
          data-layout={layout}
          lead={<Topbar s={s} compact={layout === 'compact'} />}
          body={{ className: 'lt-win-body' }}
        >
          <div className="lt-body">
            {layout === 'wide' && <Rail active={s.view === 'hosts'} />}
            <main className="lt-main">
              <section className="lt-view lt-hosts" data-show={s.view === 'hosts' || undefined} aria-hidden>
                <HostsView compact={layout === 'compact'} />
              </section>
              <section className="lt-view lt-term" data-show={s.view === 'term' || undefined} aria-hidden>
                <Terminal s={s} />
              </section>
              <Popover show={s.palette} origin="top" className="lt-pal">
                <Palette query={s.query} compact={layout === 'compact'} />
              </Popover>
            </main>
          </div>
        </Window>
      )}
    </Stage>
  )
}

function Topbar({ s, compact }: { s: S; compact: boolean }) {
  return (
    <div className="lt-top">
      {!compact && (
        <span className="lt-top-btn" aria-hidden>
          <Menu {...ICON} />
        </span>
      )}
      <div className="lt-tabs">
        <span className="lt-tab" data-id="sftp">
          <FolderTree size={14} strokeWidth={1.5} />
          SFTP
        </span>
        <span className="lt-tabwrap" data-open={s.tab || undefined}>
          <span className="lt-tabclip">
            <span className="lt-tab" data-active={s.view === 'term' || undefined}>
              <span className="lt-dot" data-state={s.live ? 'live' : 'connecting'} />
              prod-api
              <X size={12} strokeWidth={2} className="lt-tab-x" />
            </span>
          </span>
        </span>
        <span className="lt-top-btn" aria-hidden>
          <Plus size={15} strokeWidth={1.5} />
        </span>
      </div>
    </div>
  )
}

function Rail({ active }: { active: boolean }) {
  return (
    <nav className="lt-rail" aria-hidden>
      <div className="lt-rail-list">
        {RAIL.map(({ label, icon: Icon }, i) => (
          <span key={label} className="lt-nav" data-active={(i === 0 && active) || undefined}>
            <Icon {...ICON} />
            {label}
          </span>
        ))}
      </div>
      <span className="lt-nav lt-nav--settings">
        <Settings {...ICON} />
        Settings
      </span>
    </nav>
  )
}

function HostsView({ compact }: { compact: boolean }) {
  // The phone crop shows a square of the window: the first group, two hosts
  const groups = compact ? [{ ...GROUPS[0], hosts: GROUPS[0].hosts.slice(0, 2) }] : GROUPS
  return (
    <div className="lt-hosts-inner">
      <header className="lt-head">
        <div className="lt-title">
          Hosts <span className="lt-count">{HOSTS.length}</span>
        </div>
        <span className="lt-search">
          <Search size={14} strokeWidth={1.5} />
          Search hosts
        </span>
        <Button variant="primary" size="small" icon={<Plus size={12} strokeWidth={2} />} className="lt-new">
          New host
        </Button>
      </header>
      {groups.map((g) => (
        <section key={g.name} className="lt-group">
          <div className="lt-group-head">{g.name}</div>
          <div className="lt-grid">
            {g.hosts.map((h) => (
              <div key={h.label} className="lt-card">
                <span className="lt-tile">
                  <Server size={16} strokeWidth={1.5} />
                </span>
                <span className="lt-card-text">
                  <span className="lt-card-name">{h.label}</span>
                  <span className="lt-card-addr">
                    {h.user}@{h.address}
                  </span>
                </span>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  )
}

function Palette({ query, compact }: { query: string; compact: boolean }) {
  const q = query.trim().toLowerCase()
  // On a phone: three rows, so the palette clears the keycaps; one of them
  // still drops out as "prod" is typed
  const rows = compact ? [HOSTS[0], HOSTS[1], HOSTS[3]] : HOSTS
  const first = rows.findIndex((h) => h.label.includes(q))
  const addr = (h: Host) => `${h.user}@${h.address}`
  return (
    <div className="lt-pal-card">
      <div className="lt-pal-input">
        <span className="lt-pal-icon">
          <Search size={16} strokeWidth={1.5} />
        </span>
        <span className="lt-pal-field">
          {/* The placeholder fades where it is; typing never swaps one text for another */}
          <span className="lt-pal-ph" data-hide={!!query || undefined}>
            Search hosts or type user@host
          </span>
          <span className="lt-pal-typed" data-typing>
            {query}
          </span>
        </span>
      </div>
      <div className="lt-pal-list">
        {rows.map((h, i) => (
          <Collapse key={h.label} open={h.label.includes(q)}>
            <div className="lt-pal-row" data-active={i === first || undefined}>
              <span className="lt-tile lt-tile--sm">
                <Server size={14} strokeWidth={1.5} />
              </span>
              <span className="lt-pal-name">{h.label}</span>
              <span className="lt-pal-addr">{addr(h)}</span>
              <span className="lt-pal-enter" data-on={i === first || undefined}>
                <CornerDownLeft size={13} strokeWidth={1.75} />
              </span>
            </div>
          </Collapse>
        ))}
      </div>
    </div>
  )
}

/** Ubuntu's stock bash prompt: green user@host, blue path */
function Ps1() {
  return (
    <span className="lt-ps1">
      <span className="lt-ps1-host">ubuntu@prod-api</span>:<span className="lt-ps1-path">~</span>$
    </span>
  )
}

function Terminal({ s }: { s: S }) {
  return (
    <div className="lt-term-inner">
      {/* Something is on screen from the first frame the pane opens */}
      <div className="lt-connecting" data-on={!s.motd || undefined}>
        Connecting to prod-api (10.0.4.12)…
      </div>
      {/* Ubuntu's stock login banner: the lines a real session prints before the prompt */}
      <div className="lt-motd" data-on={s.motd || undefined}>
        <div>Welcome to Ubuntu 24.04.1 LTS (GNU/Linux 6.8.0-45-generic x86_64)</div>
        <div className="lt-motd-more">
          {'\n'}
          {' * Documentation:  https://help.ubuntu.com\n'}
          {' * Management:     https://landscape.canonical.com\n'}
          {' * Support:        https://ubuntu.com/pro\n'}
          {'\n'}
          {' System information as of Sun Nov  8 10:41:52 UTC 2026\n'}
          {'\n'}
          {'  System load:  0.08               Processes:             112\n'}
          {'  Usage of /:   41.2% of 38.70GB   Users logged in:       1\n'}
          {'  Memory usage: 37%                IPv4 address for eth0: 10.0.4.12\n'}
          {'  Swap usage:   0%\n'}
        </div>
        <div className="lt-dim">{'\n'}Last login: Sat Nov  7 22:14:03 2026 from 203.0.113.7</div>
      </div>
      <div className="lt-line" data-on={s.prompt || undefined}>
        <Ps1 />{' '}
        <span data-typing>
          {s.cmd}
          {!s.out && <Caret still={s.cmd.length > 0 && s.cmd !== 'uptime'} />}
        </span>
      </div>
      <div className="lt-out" data-on={s.out || undefined}>
        {' '}10:42:07 up 41 days,  3:12,  1 user,  load average: 0.08, 0.11, 0.09
      </div>
      <div className="lt-line" data-on={s.out || undefined}>
        <Ps1 /> {s.out && <Caret />}
      </div>
    </div>
  )
}
