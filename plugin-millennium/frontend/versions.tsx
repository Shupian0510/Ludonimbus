// The version history of a game's cloud saves, shown in a Steam dialog when
// the Ludonimbus status item is clicked. Restoring runs in the Ludonimbus app;
// the play bar follows its progress like any other sync.

import { DialogBody, DialogButton, DialogButtonPrimary, DialogFooter, DialogHeader, ModalRoot, showModal } from 'millennium';
import { useEffect, useState } from 'react';
import { bytes, dateTime, parseReply, Snapshot, SnapshotList, t } from './status';

type Load = { state: 'loading' } | { state: 'error'; message: string } | { state: 'ok'; list: SnapshotList };

function errorMessage(reply: unknown): string | null {
	const r = reply as { error?: string; status?: number; detail?: string | null };
	if (!r || !r.error) return null;
	if (r.status === 404 && !r.detail) return t().needUpdate;
	return r.detail ?? r.error;
}

function Row({
	s,
	list,
	confirming,
	onRestore,
	onConfirm,
	onCancel,
}: {
	s: Snapshot;
	list: SnapshotList;
	confirming: boolean;
	onRestore: () => void;
	onConfirm: () => void;
	onCancel: () => void;
}) {
	const m = t();
	const isHead = s.id === list.headId;
	const isCurrent = s.id === list.lastSyncedId;
	return (
		<div className="ludonimbus-version" data-current={isCurrent ? '1' : '0'}>
			<div className="ludonimbus-version-main">
				<div className="ludonimbus-version-title">
					<span>{dateTime(s.createdAt)}</span>
					<span className="ludonimbus-version-device">
						{s.deviceName}
						{s.deviceId === list.deviceId ? m.thisPc : ''}
					</span>
					{isHead && <span className="ludonimbus-tag">{m.latest}</span>}
					{isCurrent && <span className="ludonimbus-tag ludonimbus-tag-blue">{m.current}</span>}
					{s.locked && <span className="ludonimbus-tag">{m.locked}</span>}
				</div>
				<div className="ludonimbus-version-sub">
					{m.trigger[s.trigger] ?? s.trigger} · {m.files(s.fileCount, bytes(s.totalSize))}
					{s.comment ? ` · “${s.comment}”` : ''}
				</div>
				{confirming && (
					<div className="ludonimbus-version-confirm">
						<div>
							<b>{m.restoreTitle}</b> {m.restoreText(s.deviceName, dateTime(s.createdAt))} {m.restoreSafe}
						</div>
						<div className="ludonimbus-version-actions">
							<DialogButtonPrimary onClick={onConfirm}>{m.confirmRestore}</DialogButtonPrimary>
							<DialogButton onClick={onCancel}>{m.cancel}</DialogButton>
						</div>
					</div>
				)}
			</div>
			{!confirming && !isCurrent && (
				<DialogButton className="ludonimbus-version-restore" disabled={!!list.busy} onClick={onRestore}>
					{m.restore}
				</DialogButton>
			)}
		</div>
	);
}

function VersionsDialog({
	gameKey,
	name,
	closeModal,
	onStarted,
}: {
	gameKey: string;
	name: string;
	closeModal?: () => void;
	onStarted: () => void;
}) {
	const m = t();
	const [load, setLoad] = useState<Load>({ state: 'loading' });
	const [confirm, setConfirm] = useState<string | null>(null);
	const [message, setMessage] = useState<string | null>(null);
	const [working, setWorking] = useState(false);

	useEffect(() => {
		backend
			.list_snapshots(gameKey)
			.then((raw) => {
				const reply = parseReply(raw);
				const err = errorMessage(reply);
				if (err) setLoad({ state: 'error', message: err });
				else setLoad({ state: 'ok', list: reply as unknown as SnapshotList });
			})
			.catch((e) => setLoad({ state: 'error', message: String(e) }));
	}, [gameKey]);

	const restore = async (id: string) => {
		setWorking(true);
		setMessage(m.restoring);
		const err = errorMessage(parseReply(await backend.restore(gameKey, id)));
		setWorking(false);
		if (err) {
			setMessage(m.cantRestore(err));
			setConfirm(null);
			return;
		}
		onStarted();
		closeModal?.();
	};

	const syncNow = async () => {
		const err = errorMessage(parseReply(await backend.sync_now(gameKey)));
		if (err) {
			setMessage(err);
			return;
		}
		onStarted();
		closeModal?.();
	};

	const list = load.state === 'ok' ? load.list : null;
	return (
		<ModalRoot onCancel={closeModal} closeModal={closeModal} modalClassName="ludonimbus-versions-modal">
			<DialogHeader>{m.versionsTitle(list?.name ?? name)}</DialogHeader>
			<DialogBody>
				{load.state === 'loading' && <div className="ludonimbus-muted">{m.loadingVersions}</div>}
				{load.state === 'error' && <div className="ludonimbus-error">{load.message}</div>}
				{list && list.busy && <div className="ludonimbus-muted">{m.cantRestore(list.busy)}</div>}
				{list && list.snapshots.length === 0 && <div className="ludonimbus-muted">{m.noVersions}</div>}
				{list && list.snapshots.length > 0 && (
					<div className="ludonimbus-versions">
						{list.snapshots.map((s) => (
							<Row
								key={s.id}
								s={s}
								list={list}
								confirming={confirm === s.id}
								onRestore={() => setConfirm(s.id)}
								onConfirm={() => !working && restore(s.id)}
								onCancel={() => setConfirm(null)}
							/>
						))}
					</div>
				)}
				{message && <div className="ludonimbus-muted ludonimbus-message">{message}</div>}
			</DialogBody>
			<DialogFooter>
				<div className="ludonimbus-versions-footer">
					<DialogButton onClick={syncNow} disabled={!!list?.busy}>
						{m.syncNow}
					</DialogButton>
					<DialogButton
						onClick={() => {
							backend.open_game(gameKey);
							closeModal?.();
						}}
					>
						{m.openApp}
					</DialogButton>
					<DialogButtonPrimary onClick={closeModal}>{m.close}</DialogButtonPrimary>
				</div>
			</DialogFooter>
		</ModalRoot>
	);
}

/** Show the version history over the given Steam window. */
export function showVersions(win: Window | null, gameKey: string, name: string, onStarted: () => void): void {
	showModal(<VersionsDialog gameKey={gameKey} name={name} onStarted={onStarted} />, win ?? window, {
		strTitle: t().versionsTitle(name),
		bNeverPopOut: true,
	});
}
