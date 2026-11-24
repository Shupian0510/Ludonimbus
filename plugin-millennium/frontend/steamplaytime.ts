// Shows the playtime Ludonimbus recorded on all devices as Steam's own
// playtime of non-Steam shortcuts.
//
// Steam's library reads `minutes_playtime_forever` from its app overviews
// (`window.appStore.GetAppOverviewByAppID`, a MobX observable), so setting it
// makes the game page, the library hover and "sort by playtime" show the
// total. Only what Steam displays changes: Steam keeps counting on its own and
// nothing is written to its files. When Steam updates the value itself (after
// a session it counted), that becomes its own value again; the higher of the
// two is shown, so time Ludonimbus didn't see is never hidden. Steam's values
// are put back when the plugin stops.

import { currentAccount } from './playbar';

const POLL_MS = 30_000;

interface Overview {
	minutes_playtime_forever?: number;
}

interface Total {
	appid: number;
	minutes: number;
}

export class SteamPlaytime {
	private timer: number | undefined;
	/** Steam's own latest value per app id. */
	private readonly steam = new Map<number, number>();
	/** The value this plugin last set per app id. */
	private readonly shown = new Map<number, number>();

	start(): void {
		this.tick();
	}

	stop(): void {
		window.clearTimeout(this.timer);
		this.timer = undefined;
		for (const appid of Array.from(this.shown.keys())) this.restore(appid);
	}

	private overview(appid: number): Overview | null {
		try {
			return (window as any).appStore?.GetAppOverviewByAppID?.(appid) ?? null;
		} catch {
			return null;
		}
	}

	private async tick(): Promise<void> {
		try {
			const raw: unknown = await backend.get_playtimes(currentAccount());
			const v = (typeof raw === 'string' ? JSON.parse(raw) : raw) as { apps?: Total[] } | null;
			// Ludonimbus not running: keep what is shown until it is back.
			if (v && Array.isArray(v.apps)) this.apply(v.apps);
		} catch {
			/* keep what is shown */
		}
		this.timer = window.setTimeout(() => this.tick(), POLL_MS);
	}

	/** Exposed for tests. */
	apply(apps: Total[]): void {
		const listed = new Set<number>();
		for (const a of apps) {
			const o = this.overview(a.appid);
			if (!o) continue;
			listed.add(a.appid);
			const current = o.minutes_playtime_forever ?? 0;
			// Not what we set: Steam's own value (first time, or Steam updated it).
			if (this.shown.get(a.appid) !== current) this.steam.set(a.appid, current);
			const show = Math.max(a.minutes, this.steam.get(a.appid) ?? 0);
			if (current !== show) {
				try {
					o.minutes_playtime_forever = show;
				} catch {
					continue;
				}
			}
			this.shown.set(a.appid, show);
		}
		// No longer a Ludonimbus game (removed, unlinked): Steam's value again.
		for (const appid of Array.from(this.shown.keys())) if (!listed.has(appid)) this.restore(appid);
	}

	private restore(appid: number): void {
		const o = this.overview(appid);
		const own = this.steam.get(appid);
		if (o && own !== undefined && o.minutes_playtime_forever === this.shown.get(appid)) {
			try {
				o.minutes_playtime_forever = own;
			} catch {
				/* leave it */
			}
		}
		this.shown.delete(appid);
		this.steam.delete(appid);
	}
}
