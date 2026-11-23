// Ludonimbus Steam plugin: shows the cloud save status of non-Steam games
// managed by Ludonimbus in the play bar of their library page.

import { definePlugin, Field, Millennium } from 'millennium';
import { useEffect, useState } from 'react';
import { PlayBarStatus } from './playbar';
import { parseReply } from './status';

const bars = new Map<Document, PlayBarStatus>();

function attach(name: string | undefined, doc: Document | undefined): void {
	// Only the desktop library window ("SP Desktop_uid0"), not Big Picture.
	if (!name?.startsWith('SP Desktop') || !doc?.body || bars.has(doc)) return;
	for (const [old, bar] of bars) {
		if (!old.defaultView || old.defaultView.closed) {
			bar.stop();
			bars.delete(old);
		}
	}
	const bar = new PlayBarStatus(doc);
	bars.set(doc, bar);
	bar.start();
}

/** Windows that already existed when the plugin loaded. */
function attachExisting(): void {
	try {
		const popups: Map<string, any> | undefined = (window as any).g_PopupManager?.m_mapPopups;
		popups?.forEach((p: any) => attach(p?.m_strName, p?.m_popup?.document));
	} catch {
		/* not available in this Steam build */
	}
}

const QuickAccess = () => {
	const [state, setState] = useState('正在连接 Ludonimbus…');
	useEffect(() => {
		backend
			.ping()
			.then((raw) => {
				const v = parseReply(raw) as { version?: string };
				setState(v.version ? `已连接 Ludonimbus ${v.version}` : 'Ludonimbus 未运行，打开它后显示云存档状态');
			})
			.catch(() => setState('Ludonimbus 未运行，打开它后显示云存档状态'));
	}, []);
	return <Field label="Ludonimbus 云存档" description={state} />;
};

export default definePlugin(() => {
	Millennium.AddWindowCreateHook?.((ctx: any) => attach(ctx?.m_strName, ctx?.m_popup?.document));
	attachExisting();

	return {
		title: 'Ludonimbus 云存档',
		icon: <div style={{ width: 16, height: 16 }}>☁</div>,
		content: <QuickAccess />,
	};
});
