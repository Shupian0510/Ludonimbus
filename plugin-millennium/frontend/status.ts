// What the local API returns and how it is shown in the play bar.

export type Phase = 'idle' | 'syncing' | 'preLaunch' | 'playing' | 'uploading';

export type Status =
	| 'noCloud'
	| 'empty'
	| 'notUploaded'
	| 'synced'
	| 'localChanged'
	| 'cloudNewer'
	| 'canRestore'
	| 'conflict'
	| 'offline'
	| 'error';

export interface GameStatus {
	managed: true;
	key: string;
	name: string;
	autoSync: boolean;
	steamEnabled: boolean;
	health: string;
	phase: Phase;
	status: Status | null;
	message: string | null;
	lastSyncAt: string | null;
	lastSyncedDevice: string | null;
	cloudHead: { deviceName: string; createdAt: string } | null;
	pending: number;
	conflict: boolean;
	lastError: string | null;
	checkedAt: string | null;
	/** Ludonimbus's interface language. */
	lang?: string;
	/** 0.6.0 and later. */
	playtime?: Playtime;
	apiVersion?: number;
	/** API 4: a game Steam itself installed, and who keeps its saves. */
	store?: StoreState | null;
	/** API 4: what is left to do in the background after playing. */
	backlog?: Backlog | null;
}

/** Steam Cloud keeps the saves / Ludonimbus stores versions of them / Ludonimbus keeps them. */
export type StoreState = 'steam' | 'backup' | 'managed';

export interface Backlog {
	/** A sync of the game runs right now, or nobody is at it until later. */
	state: 'uploading' | 'waiting';
	versions: number;
	bytes: number;
}

export interface PlaytimeDevice {
	device: string;
	deviceName: string;
	secs: number;
	lastPlayed: string | null;
	thisDevice: boolean;
	steamMinutes: number | null;
}

export interface Playtime {
	totalSecs: number;
	lastPlayed: string | null;
	devices: PlaytimeDevice[];
}

export interface Snapshot {
	id: string;
	deviceId: string;
	deviceName: string;
	createdAt: string;
	trigger: string;
	totalSize: number;
	fileCount: number;
	comment: string;
	locked: boolean;
	/** API 4: what it was saved on, e.g. "pc:steam". */
	platform?: string;
}

export interface SnapshotList {
	key: string;
	name: string;
	snapshots: Snapshot[];
	headId: string | null;
	lastSyncedId: string | null;
	deviceId: string;
	pending: number;
	/** Why restoring is not possible right now (game running, syncing). */
	busy: string | null;
}

export type Reply = GameStatus | { managed: false } | { error: string; status?: number; detail?: string | null };

export type Tone = 'ok' | 'info' | 'warn' | 'bad' | 'muted';

export interface View {
	text: string;
	tone: Tone;
	tip: string;
	/** Show the "sync now" button. */
	canSync: boolean;
	/** Animate the icon (work in progress). */
	busy: boolean;
	/** Clicking opens the game in Ludonimbus. */
	key: string | null;
	/** What a click does: show the version history, open Ludonimbus (conflicts, repairs), or open its restore dialog for the game files. */
	action?: 'versions' | 'open' | 'install';
}

/** `GET /v1/games/{key}/gamebackup`. */
export interface BackupStatus {
	exeMissing: boolean;
	latest: { versionId: string; createdAt: string; totalSize: number; deviceName: string } | null;
	job: { kind: string; phase: string; done: number; total: number } | null;
}

export function parseBackup(value: unknown): BackupStatus | null {
	const r = parseReply(value) as unknown as Partial<BackupStatus> & { error?: string };
	return r && !r.error && typeof r.exeMissing === 'boolean' ? (r as BackupStatus) : null;
}

/** The play bar item while the game's files are missing or being restored. */
export function describeInstall(b: BackupStatus | null, key: string): View | null {
	if (!b) return null;
	const m = t();
	if (b.job?.kind === 'restore') {
		const pct = b.job.total > 0 && b.job.phase === 'downloading' ? Math.floor((b.job.done / b.job.total) * 100) : null;
		return { text: m.installing(pct), tone: 'info', tip: m.clickToOpen, canSync: false, busy: true, key, action: 'open' };
	}
	if (b.exeMissing && b.latest) {
		const gb = (b.latest.totalSize / 1024 ** 3).toFixed(1);
		return {
			text: m.installFromCloud,
			tone: 'info',
			tip: m.installTip(dateTime(b.latest.createdAt), b.latest.deviceName, gb),
			canSync: false,
			busy: false,
			key,
			action: 'install',
		};
	}
	return null;
}

/**
 * The backend returns JSON text, but Millennium's FFI parses JSON results
 * before handing them over, so accept both.
 */
export function parseReply(value: unknown): Reply {
	let v = value;
	if (typeof value === 'string') {
		try {
			v = JSON.parse(value);
		} catch {
			return { error: 'bad_reply', detail: value.slice(0, 200) };
		}
	}
	if (v && typeof v === 'object') return v as Reply;
	return { error: 'bad_reply', detail: String(v) };
}

export function isGame(r: Reply | null): r is GameStatus {
	return !!r && 'managed' in r && r.managed === true;
}

/** Steam gives every non-Steam shortcut an app id with the top bit set. */
export function isShortcutAppId(appId: string): boolean {
	const n = Number(appId);
	return Number.isFinite(n) && n >= 0x80000000;
}

// ---------------------------------------------------------------------------
// Texts. The language follows Ludonimbus (every reply says which one); until
// the first reply, Steam's own language decides.

type Lang = 'zh-CN' | 'en';
let lang: Lang = (navigator.language || '').toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';

export function setLang(l: string | undefined): void {
	if (l === 'zh-CN' || l === 'en') lang = l;
}

const pl = (n: number, one: string, many: string) => (n === 1 ? one : many);

const TEXTS = {
	'zh-CN': {
		label: 'Ludonimbus 云存档',
		syncNow: '立即同步',
		connecting: '正在连接 Ludonimbus…',
		connected: (v: string) => `已连接 Ludonimbus ${v}`,
		notRunningLong: 'Ludonimbus 未运行，打开它后显示云存档状态',
		never: '从未',
		justNow: '刚刚',
		minutesAgo: (n: number) => `${n} 分钟前`,
		hoursAgo: (n: number) => `${n} 小时前`,
		daysAgo: (n: number) => `${n} 天前`,
		notRunning: 'Ludonimbus 未运行',
		notRunningTip: '打开 Ludonimbus 后显示这个游戏的云存档状态。',
		cantRead: '无法读取状态',
		lastSync: (at: string, device: string | null) => `上次同步：${at}${device ? `（${device}）` : ''}`,
		neverSynced: '这台电脑还没有同步过',
		cloudHead: (at: string, device: string) => `云端最新：${at}，来自 ${device}`,
		pending: (n: number) => `${n} 个版本等待上传`,
		lastError: (e: string) => `上次出错：${e}`,
		notEnabled: '没有在 Steam 中开启自动同步：从 Steam 启动时不会自动同步',
		clickToOpen: '点击在 Ludonimbus 中查看',
		preLaunch: '正在检查云存档…',
		syncing: '正在同步…',
		uploading: '正在上传存档…',
		backlogUploading: (size: string) => `正在后台上传${size ? `（${size}）` : ''}`,
		backlogWaiting: '没连上，稍后再传',
		backlogQueued: (size: string) => `等待上传${size ? `（${size}）` : ''}`,
		playingAuto: '游戏运行中，退出后自动上传',
		playing: '游戏运行中',
		broken: '快捷方式已更改，自动同步失效，点击修复',
		brokenOptions: '启动选项已更改，自动同步失效，点击修复',
		autoOff: '已关闭自动同步',
		synced: (when: string) => `已是最新${when ? ` · ${when}` : ''}`,
		backedUp: (when: string) => `已备份${when ? ` · ${when}` : ''}`,
		localChanged: '本机有新存档，等待上传',
		notUploaded: '存档尚未上传',
		cloudNewer: (device: string | null) => (device ? `云端有更新的存档（${device}）` : '云端有更新的存档'),
		canRestore: '本机没有存档，可从云端恢复',
		conflict: '存档冲突，点击选择保留哪一份',
		offline: (pending: number) => (pending > 0 ? `云端无法访问，${pending} 个版本待上传` : '云端暂时无法访问'),
		noCloud: '还没有连接网盘',
		empty: '还没有存档',
		error: '读取存档出错',
		checking: '正在检查…',
		clickVersions: '点击查看存档',
		versionsTitle: (name: string) => `${name} · 存档`,
		loadingVersions: '正在读取存档…',
		noVersions: '云端还没有存档。玩一次游戏或点「立即同步」后就会出现。',
		latest: '最新',
		current: '当前',
		locked: '保留',
		thisPc: '（本机）',
		files: (n: number, size: string) => `${n} 个文件 · ${size}`,
		restore: '设为当前',
		restoreTitle: '设为当前？',
		restoreText: (device: string, at: string) => `这台设备上的存档会换成 ${device} 在 ${at} 的这一份，现在的会先保留。`,
		restoreSteam: 'Steam 云也在同步这个游戏：它下次同步时可能问保留哪一份，选本机的。',
		confirmRestore: '设为当前',
		cancel: '取消',
		close: '关闭',
		openApp: '在 Ludonimbus 中打开',
		restoring: '正在切换…',
		cantRestore: (reason: string) => `现在不能切换：${reason}`,
		needUpdate: '需要更新 Ludonimbus 到 0.6 或更高版本才能在 Steam 里查看存档。',
		trigger: {
			'auto-exit': '退出游戏后自动',
			'pre-launch': '启动前补传',
			passive: '后台检测到退出',
			'sync-now': '手动同步',
			manual: '手动备份',
			restore: '设为当前',
			'pre-restore': '替换前保留',
			'conflict-keep': '冲突时保留',
			shared: '别人分享的',
		} as Record<string, string>,
		installFromCloud: '本机没有游戏文件 · 点击从云端安装',
		installing: (pct: number | null) => (pct === null ? '正在从云端安装…' : `正在从云端安装 ${pct}%`),
		installTip: (at: string, device: string, gb: string) => `云端有游戏本体备份：${at}，来自 ${device}，${gb} GB
点击在 Ludonimbus 里选择安装位置`,
	},
	en: {
		label: 'Ludonimbus Cloud Saves',
		syncNow: 'Sync now',
		connecting: 'Connecting to Ludonimbus…',
		connected: (v: string) => `Connected to Ludonimbus ${v}`,
		notRunningLong: 'Ludonimbus is not running. Open it to see cloud save status',
		never: 'never',
		justNow: 'just now',
		minutesAgo: (n: number) => `${n} ${pl(n, 'minute', 'minutes')} ago`,
		hoursAgo: (n: number) => `${n} ${pl(n, 'hour', 'hours')} ago`,
		daysAgo: (n: number) => `${n} ${pl(n, 'day', 'days')} ago`,
		notRunning: 'Ludonimbus is not running',
		notRunningTip: "Open Ludonimbus to see this game's cloud save status.",
		cantRead: "Can't read the status",
		lastSync: (at: string, device: string | null) => `Last sync: ${at}${device ? ` (${device})` : ''}`,
		neverSynced: "This PC hasn't synced yet",
		cloudHead: (at: string, device: string) => `Latest in the cloud: ${at}, from ${device}`,
		pending: (n: number) => `${n} ${pl(n, 'version', 'versions')} waiting to upload`,
		lastError: (e: string) => `Last error: ${e}`,
		notEnabled: "Steam auto-sync is off: starting from Steam won't sync",
		clickToOpen: 'Click to open in Ludonimbus',
		preLaunch: 'Checking cloud saves…',
		syncing: 'Syncing…',
		uploading: 'Uploading saves…',
		backlogUploading: (size: string) => `Uploading in the background${size ? ` (${size})` : ''}`,
		backlogWaiting: "Couldn't reach the cloud; trying again later",
		backlogQueued: (size: string) => `Waiting to upload${size ? ` (${size})` : ''}`,
		playingAuto: 'Playing; saves upload when you quit',
		playing: 'Playing',
		broken: 'Shortcut changed; auto-sync is off. Click to repair',
		brokenOptions: 'Launch options changed; auto-sync is off. Click to repair',
		autoOff: 'Auto-sync is off',
		synced: (when: string) => `Up to date${when ? ` · ${when}` : ''}`,
		backedUp: (when: string) => `Backed up${when ? ` · ${when}` : ''}`,
		localChanged: 'New saves on this PC, waiting to upload',
		notUploaded: 'Saves not uploaded yet',
		cloudNewer: (device: string | null) => (device ? `Newer saves in the cloud (${device})` : 'Newer saves in the cloud'),
		canRestore: 'No saves on this PC; can restore from the cloud',
		conflict: 'Save conflict; click to choose which to keep',
		offline: (pending: number) =>
			pending > 0 ? `Cloud unreachable; ${pending} ${pl(pending, 'version', 'versions')} waiting` : "The cloud can't be reached right now",
		noCloud: 'No cloud connected yet',
		empty: 'No saves yet',
		error: 'Error reading saves',
		checking: 'Checking…',
		clickVersions: 'Click to see the saves',
		versionsTitle: (name: string) => `${name} · Saves`,
		loadingVersions: 'Reading the saves…',
		noVersions: 'No saves in the cloud yet. Play once or press "Sync now" to create the first one.',
		latest: 'Latest',
		current: 'Current',
		locked: 'Kept',
		thisPc: ' (this PC)',
		files: (n: number, size: string) => `${n} ${pl(n, 'file', 'files')} · ${size}`,
		restore: 'Make current',
		restoreTitle: 'Make this the current save?',
		restoreText: (device: string, at: string) => `The saves on this device are replaced by the ones ${device} saved at ${at}; the ones here are kept first.`,
		restoreSteam: "Steam Cloud syncs this game too: next time it may ask which saves to keep. Pick this PC's.",
		confirmRestore: 'Make current',
		cancel: 'Cancel',
		close: 'Close',
		openApp: 'Open in Ludonimbus',
		restoring: 'Switching…',
		cantRestore: (reason: string) => `Can't switch right now: ${reason}`,
		needUpdate: 'Update Ludonimbus to 0.6 or later to see the saves in Steam.',
		trigger: {
			'auto-exit': 'After the game exited',
			'pre-launch': 'Uploaded before launch',
			passive: 'Exit seen in the background',
			'sync-now': 'Synced by hand',
			manual: 'Manual backup',
			restore: 'Made current',
			'pre-restore': 'Kept before replacing',
			'conflict-keep': 'Kept in a conflict',
			shared: 'Shared by someone',
		} as Record<string, string>,
		installFromCloud: 'Game files missing · Click to install from the cloud',
		installing: (pct: number | null) => (pct === null ? 'Installing from the cloud…' : `Installing from the cloud ${pct}%`),
		installTip: (at: string, device: string, gb: string) =>
			`The cloud has a backup of the game files: ${at}, from ${device}, ${gb} GB
Click to choose where to install it in Ludonimbus`,
	},
};

export function t() {
	return TEXTS[lang];
}

export function relTime(iso: string | null | undefined): string {
	const m = t();
	if (!iso) return m.never;
	const diff = (Date.now() - new Date(iso).getTime()) / 1000;
	if (diff < 45) return m.justNow;
	if (diff < 3600) return m.minutesAgo(Math.round(diff / 60));
	if (diff < 86400) return m.hoursAgo(Math.round(diff / 3600));
	if (diff < 86400 * 30) return m.daysAgo(Math.round(diff / 86400));
	return dateTime(iso);
}

export function dateTime(iso: string): string {
	const d = new Date(iso);
	const p = (n: number) => String(n).padStart(2, '0');
	return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

const BROKEN_SHORTCUT = ['changed', 'runnerMoved', 'shortcutMissing'];
/** The same for the launch options of a game Steam installed. */
const BROKEN_OPTIONS = ['changed', 'runnerMoved', 'missing'];

function broken(g: GameStatus): boolean {
	return (g.store ? BROKEN_OPTIONS : BROKEN_SHORTCUT).includes(g.health);
}

/** `null` = show nothing (a regular Steam game, or not known yet). */
export function describe(reply: Reply | null, appId: string): View | null {
	if (!reply) return null;
	if ('managed' in reply && reply.managed) setLang(reply.lang);
	const m = t();
	if ('error' in reply) {
		if (!isShortcutAppId(appId)) return null;
		if (reply.error === 'not_running') {
			return {
				text: m.notRunning,
				tone: 'muted',
				tip: m.notRunningTip,
				canSync: false,
				busy: false,
				key: null,
			};
		}
		return {
			text: m.cantRead,
			tone: 'muted',
			tip: reply.detail ?? reply.error,
			canSync: false,
			busy: false,
			key: null,
		};
	}
	if (!reply.managed) return null;

	const g = reply;
	// Steam Cloud keeps this game's saves and Ludonimbus does nothing with them.
	if (g.store === 'steam') return null;
	const tips: string[] = [];
	if (g.lastSyncAt) {
		tips.push(m.lastSync(dateTime(g.lastSyncAt), g.lastSyncedDevice));
	} else {
		tips.push(m.neverSynced);
	}
	if (g.cloudHead) tips.push(m.cloudHead(dateTime(g.cloudHead.createdAt), g.cloudHead.deviceName));
	if (g.pending > 0) tips.push(m.pending(g.pending));
	if (g.lastError) tips.push(m.lastError(g.lastError));
	// A game Steam installed is synced without the wrapper too.
	if (!g.steamEnabled && !g.store) tips.push(m.notEnabled);
	// Conflicts and broken shortcuts are sorted out in Ludonimbus; otherwise
	// a click shows the saves (app 0.6 and later).
	const needsApp = g.status === 'conflict' || broken(g);
	const action: 'versions' | 'open' = !needsApp && (g.apiVersion ?? 1) >= 2 ? 'versions' : 'open';
	tips.push(action === 'versions' ? m.clickVersions : m.clickToOpen);
	const tip = tips.join('\n');
	const base = { tip, key: g.key, canSync: false, busy: false, action };

	switch (g.phase) {
		case 'preLaunch':
			return { ...base, text: m.preLaunch, tone: 'info', busy: true };
		case 'syncing':
			// The app finishing what the wrapper left: said as that.
			return { ...base, text: g.backlog ? m.backlogUploading(size(g.backlog)) : m.syncing, tone: 'info', busy: true };
		case 'uploading':
			return { ...base, text: m.uploading, tone: 'info', busy: true };
		case 'playing':
			return { ...base, text: g.autoSync ? m.playingAuto : m.playing, tone: 'muted' };
	}

	// Played, and the cloud doesn't have it yet: in the background.
	if (g.backlog?.state === 'uploading') {
		return { ...base, text: m.backlogUploading(size(g.backlog)), tone: 'info', busy: true };
	}
	if (g.backlog) {
		const text = g.status === 'offline' ? m.backlogWaiting : m.backlogQueued(size(g.backlog));
		return { ...base, text, tone: 'muted', canSync: true };
	}

	if (broken(g)) {
		return { ...base, text: g.store ? m.brokenOptions : m.broken, tone: 'warn' };
	}
	if (!g.autoSync) {
		return { ...base, text: m.autoOff, tone: 'muted', canSync: true };
	}

	const when = g.lastSyncAt ? relTime(g.lastSyncAt) : '';
	const canSync = true;
	switch (g.status) {
		case 'synced':
			return { ...base, text: g.store === 'backup' ? m.backedUp(when) : m.synced(when), tone: 'ok', canSync };
		case 'localChanged':
			return { ...base, text: m.localChanged, tone: 'info', canSync };
		case 'notUploaded':
			return { ...base, text: m.notUploaded, tone: 'info', canSync };
		case 'cloudNewer':
			return { ...base, text: m.cloudNewer(g.cloudHead?.deviceName ?? null), tone: 'info', canSync };
		case 'canRestore':
			return { ...base, text: m.canRestore, tone: 'info', canSync };
		case 'conflict':
			return { ...base, text: m.conflict, tone: 'warn' };
		case 'offline':
			return { ...base, text: m.offline(g.pending), tone: 'muted', canSync };
		case 'noCloud':
			return { ...base, text: m.noCloud, tone: 'muted' };
		case 'empty':
			return { ...base, text: m.empty, tone: 'muted' };
		case 'error':
			return { ...base, text: m.error, tone: 'bad', tip: `${g.message ?? ''}\n${tip}`.trim(), canSync };
		default:
			return { ...base, text: m.checking, tone: 'muted', busy: true };
	}
}

function size(b: Backlog): string {
	return b.bytes > 0 ? bytes(b.bytes) : '';
}

export function bytes(n: number): string {
	if (n < 1024) return `${n} B`;
	const units = ['KB', 'MB', 'GB', 'TB'];
	let v = n / 1024;
	let i = 0;
	while (v >= 1024 && i < units.length - 1) {
		v /= 1024;
		i++;
	}
	return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}
