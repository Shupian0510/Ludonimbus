// Steam's hands for Ludonimbus (local API 5, docs/steam-plugin.md): what
// Steam itself says about a game's Steam Cloud switch and launch options,
// and changing them when Ludonimbus asks (after the user confirmed it there).
//
// Where Steam keeps a game's Steam Cloud switch on disk isn't documented and
// Steam rewrites its files while it runs, so Ludonimbus asks Steam through
// this plugin instead (`SteamClient.Apps`):
//   - every 2 s the plugin polls the app: it says what it can do and takes
//     the requests waiting for it;
//   - a request is done by reading first (the switch is a toggle, not a
//     setter), changing only what differs, waiting for Steam to show the
//     change, and answering with what Steam says now;
//   - for the games Steam installed that are in Ludonimbus ("watch"), the
//     plugin reports what Steam says whenever it changes, so that Ludonimbus
//     notices a switch turned on again right away.
// Older apps answer the poll with "not found": the plugin then polls slowly
// and does nothing else.

import { currentAccount, parseReply } from './status';

const POLL_MS = 2000;
/** Ludonimbus isn't running, or is too old to ask anything. */
const IDLE_POLL_MS = 15000;
/** How long Steam has to show a change. */
const CHANGE_WAIT_MS = 5000;
const VERSION = '1.0.0';

/** What Steam's app details say about Steam Cloud and the launch options. */
interface Details {
	bCloudAvailable: boolean;
	bCloudEnabledForAccount: boolean;
	bCloudEnabledForApp: boolean;
	eCloudStatus?: number;
	strLaunchOptions?: string;
}

interface Ask {
	id: number;
	appid: number;
	kind: 'read' | 'cloud' | 'launchOptions';
	on?: boolean;
	value?: string;
}

interface Poll {
	requests?: Ask[];
	watch?: number[];
	resend?: boolean;
}

type Unregister = { unregister: () => void };

function steamApps(): any {
	return (window as any).SteamClient?.Apps;
}

/** What this Steam lets the plugin do. */
export function capabilities(): string[] {
	const a = steamApps();
	if (!a?.RegisterForAppDetails) return [];
	const out: string[] = [];
	if (typeof a.ToggleAppSteamCloudEnabled === 'function') out.push('cloud');
	if (typeof a.SetAppLaunchOptions === 'function') out.push('launchOptions');
	return out;
}

/** The game's details as Steam has them now; with `until`, once it holds (at most `ms`). */
function details(appid: number, until?: (d: Details) => boolean, ms = CHANGE_WAIT_MS): Promise<Details | null> {
	return new Promise((resolve) => {
		let last: Details | null = null;
		let reg: Unregister | null = null;
		let done = false;
		const finish = (d: Details | null) => {
			if (done) return;
			done = true;
			window.clearTimeout(timer);
			try {
				reg?.unregister();
			} catch {
				/* gone already */
			}
			resolve(d);
		};
		const timer = window.setTimeout(() => finish(last), ms);
		try {
			reg = steamApps().RegisterForAppDetails(appid, (d: Details) => {
				last = d;
				if (!until || until(d)) finish(d);
			});
		} catch {
			finish(null);
		}
		// Answered while registering: let go now that the handle is known.
		if (done) {
			try {
				reg?.unregister();
			} catch {
				/* gone already */
			}
		}
	});
}

/** encodeURIComponent, with the few characters it leaves that the backend refuses. */
const enc = (v: string) => encodeURIComponent(v).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);

type Param = string | number | boolean | null | undefined;

function query(params: Record<string, Param>): string {
	return Object.entries(params)
		.filter(([, v]) => v !== undefined && v !== null)
		.map(([k, v]) => `${k}=${enc(typeof v === 'boolean' ? (v ? '1' : '0') : String(v))}`)
		.join('&');
}

function reportOf(d: Details): Record<string, Param> {
	return {
		available: !!d.bCloudAvailable,
		accountEnabled: !!d.bCloudEnabledForAccount,
		appEnabled: !!d.bCloudEnabledForApp,
		status: typeof d.eCloudStatus === 'number' ? d.eCloudStatus : undefined,
		launchOptions: typeof d.strLaunchOptions === 'string' ? d.strLaunchOptions : undefined,
	};
}

const signature = (d: Details) => JSON.stringify(reportOf(d));

const randomId = () => Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, '0')).join('');

export class SteamHands {
	/** This run of the plugin: reports count only while it keeps polling. */
	private readonly session = randomId();
	private timer: number | undefined;
	private running = false;
	private account = '';
	private readonly watching = new Map<number, Unregister>();
	private readonly latest = new Map<number, Details>();
	/** What was last reported of each game. */
	private readonly sent = new Map<number, string>();
	private flushTimer: number | undefined;
	/** The app polled answered: it understands API 5. */
	connected = false;

	start(): void {
		if (this.running || capabilities().length === 0) return;
		this.running = true;
		this.tick();
	}

	stop(): void {
		this.running = false;
		window.clearTimeout(this.timer);
		window.clearTimeout(this.flushTimer);
		this.unwatchAll();
	}

	private unwatchAll(): void {
		for (const reg of this.watching.values()) {
			try {
				reg.unregister();
			} catch {
				/* gone already */
			}
		}
		this.watching.clear();
		this.latest.clear();
		this.sent.clear();
	}

	private async tick(): Promise<void> {
		let next = POLL_MS;
		try {
			const account = currentAccount();
			if (account !== this.account) {
				// Another account signed in: its games, reported anew.
				this.account = account;
				this.unwatchAll();
			}
			const params = { session: this.session, kind: 'millennium', version: VERSION, caps: capabilities().join(','), account };
			const reply = parseReply(await backend.plugin_poll(query(params)));
			if ('error' in reply) {
				// Not running, or an app before API 5: everything is reported again later.
				this.connected = false;
				this.sent.clear();
				next = IDLE_POLL_MS;
			} else {
				this.connected = true;
				const poll = reply as unknown as Poll;
				if (poll.resend) this.sent.clear();
				this.watch(poll.watch ?? []);
				for (const ask of poll.requests ?? []) await this.handle(ask);
				await this.flush();
			}
		} catch {
			this.connected = false;
			this.sent.clear();
			next = IDLE_POLL_MS;
		}
		if (this.running) this.timer = window.setTimeout(() => this.tick(), next);
	}

	/** Follow Steam's details of these games; let go of the others. */
	private watch(appids: number[]): void {
		const want = new Set(appids.map(Number).filter((n) => Number.isFinite(n) && n > 0));
		for (const [appid, reg] of this.watching) {
			if (want.has(appid)) continue;
			try {
				reg.unregister();
			} catch {
				/* gone already */
			}
			this.watching.delete(appid);
			this.latest.delete(appid);
			this.sent.delete(appid);
		}
		for (const appid of want) {
			if (this.watching.has(appid)) continue;
			try {
				const reg = steamApps().RegisterForAppDetails(appid, (d: Details) => {
					this.latest.set(appid, d);
					this.flushSoon();
				});
				this.watching.set(appid, reg);
			} catch {
				/* Steam has no details of it */
			}
		}
	}

	private flushSoon(): void {
		window.clearTimeout(this.flushTimer);
		this.flushTimer = window.setTimeout(() => void this.flush(), 300);
	}

	/** Report the games whose details changed since they were last reported. */
	private async flush(): Promise<void> {
		if (!this.connected) return;
		for (const [appid, d] of this.latest) {
			const sig = signature(d);
			if (this.sent.get(appid) === sig) continue;
			const reply = parseReply(await backend.plugin_report(String(appid), query({ session: this.session, account: this.account, ...reportOf(d) })));
			if (!('error' in reply)) this.sent.set(appid, sig);
		}
	}

	/** Do what the app asks: read first, change only what differs, read again. */
	private async handle(ask: Ask): Promise<void> {
		const appid = Number(ask.appid);
		let d = await details(appid);
		let ok = false;
		let error = '';
		try {
			if (!d) throw new Error('Steam has no details of this game');
			if (ask.kind === 'read') {
				ok = true;
			} else if (ask.kind === 'cloud') {
				const want = !!ask.on;
				if (!d.bCloudAvailable || !!d.bCloudEnabledForApp === want) {
					ok = true;
				} else {
					steamApps().ToggleAppSteamCloudEnabled(appid);
					d = (await details(appid, (x) => !!x.bCloudEnabledForApp === want)) ?? d;
					ok = !!d.bCloudEnabledForApp === want;
					if (!ok) error = 'Steam Cloud switch did not change';
				}
			} else if (ask.kind === 'launchOptions') {
				const want = ask.value ?? '';
				if ((d.strLaunchOptions ?? '') === want) {
					ok = true;
				} else {
					steamApps().SetAppLaunchOptions(appid, want);
					d = (await details(appid, (x) => (x.strLaunchOptions ?? '') === want)) ?? d;
					ok = (d.strLaunchOptions ?? '') === want;
					if (!ok) error = 'launch options did not change';
				}
			} else {
				error = `unknown request ${String(ask.kind)}`;
			}
		} catch (e) {
			ok = false;
			error = String(e instanceof Error ? e.message : e).slice(0, 300);
		}
		if (d && this.watching.has(appid)) {
			this.latest.set(appid, d);
			this.sent.set(appid, signature(d));
		}
		await backend.plugin_done(String(ask.id), query({ session: this.session, ok, error: error || undefined, ...(d ? reportOf(d) : {}) }));
	}
}

/** The one for the plugin (started in index.tsx). */
export const hands = new SteamHands();
