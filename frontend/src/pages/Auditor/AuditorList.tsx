import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Ban, Copy } from 'lucide-react';
import { Card, LoadingSpinner, Modal } from '../../components/Common';
import { auditorAdminService } from '../../services/api';
import type { AuditorSession, CreateAuditorRequest, CreateAuditorResponse } from '../../services/api/viewing-key';

/**
 * F1.1 Admin-side — Auditor management (create / list / deactivate).
 * Reachable from Settings. Auditor itself logs in via separate /auditor/login.
 */
export function AuditorList() {
  const { t } = useTranslation();
  const [list, setList] = useState<AuditorSession[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [created, setCreated] = useState<CreateAuditorResponse | null>(null);
  const [draft, setDraft] = useState<CreateAuditorRequest>({
    email: '',
    name: '',
    wallet_ids: [],
    scope_start: undefined,
    scope_end: undefined,
    max_count: 50,
  });

  async function load() {
    setLoading(true);
    try {
      setList(await auditorAdminService.list());
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => { load(); }, []);

  async function onCreate() {
    try {
      const resp = await auditorAdminService.create(draft);
      setCreated(resp);
      setShowCreate(false);
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  async function onDeactivate(id: number) {
    if (!confirm(t('auditor.list.confirm_deactivate'))) return;
    try {
      await auditorAdminService.deactivate(id);
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('auditor.list.title')}</h1>
        <button className="btn-primary" onClick={() => setShowCreate(true)}>
          <Plus className="w-4 h-4 inline mr-1" /> {t('auditor.list.new')}
        </button>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-xs text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">{t('auditor.list.col.email')}</th>
              <th className="text-left p-2">{t('auditor.list.col.name')}</th>
              <th className="text-left p-2">{t('auditor.list.col.scope_start')}</th>
              <th className="text-left p-2">{t('auditor.list.col.scope_end')}</th>
              <th className="text-right p-2">{t('auditor.list.col.max_count')}</th>
              <th className="text-left p-2">{t('auditor.list.col.last_login')}</th>
              <th className="text-right p-2">{t('common.action')}</th>
            </tr>
          </thead>
          <tbody>
            {list.map(a => (
              <tr key={a.id} className="border-t border-gray-100">
                <td className="p-2">{a.email}</td>
                <td className="p-2">{a.name}</td>
                <td className="p-2 text-xs">{a.scope_start ? new Date(a.scope_start).toLocaleDateString() : '—'}</td>
                <td className="p-2 text-xs">{a.scope_end ? new Date(a.scope_end).toLocaleDateString() : '—'}</td>
                <td className="p-2 text-right">{a.max_count ?? '∞'}</td>
                <td className="p-2 text-xs">{a.last_login_at ? new Date(a.last_login_at).toLocaleString() : '—'}</td>
                <td className="p-2 text-right">
                  <button className="btn-ghost" onClick={() => onDeactivate(a.id)} title={t('auditor.list.deactivate')}>
                    <Ban className="w-4 h-4 text-red-600" />
                  </button>
                </td>
              </tr>
            ))}
            {list.length === 0 && (
              <tr><td colSpan={7} className="p-4 text-center text-gray-400">{t('auditor.list.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>

      <Modal isOpen={showCreate} onClose={() => setShowCreate(false)} title={t('auditor.list.new')}>
        <div className="space-y-3 text-sm">
          <label className="block">
            <span className="text-gray-600">{t('auditor.list.col.email')}</span>
            <input className="mt-1 w-full border rounded p-2" value={draft.email} onChange={e => setDraft({ ...draft, email: e.target.value })} />
          </label>
          <label className="block">
            <span className="text-gray-600">{t('auditor.list.col.name')}</span>
            <input className="mt-1 w-full border rounded p-2" value={draft.name} onChange={e => setDraft({ ...draft, name: e.target.value })} />
          </label>
          <label className="block">
            <span className="text-gray-600">{t('auditor.list.scope_wallets')}</span>
            <input
              className="mt-1 w-full border rounded p-2 font-mono"
              placeholder="1,2,3"
              onChange={e => setDraft({ ...draft, wallet_ids: e.target.value.split(',').map(s => Number(s.trim())).filter(Number.isFinite) })}
            />
          </label>
          <div className="grid grid-cols-2 gap-2">
            <label>
              <span className="text-gray-600">{t('auditor.list.col.scope_start')}</span>
              <input type="datetime-local" className="mt-1 w-full border rounded p-2" onChange={e => setDraft({ ...draft, scope_start: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('auditor.list.col.scope_end')}</span>
              <input type="datetime-local" className="mt-1 w-full border rounded p-2" onChange={e => setDraft({ ...draft, scope_end: e.target.value })} />
            </label>
          </div>
          <label className="block">
            <span className="text-gray-600">{t('auditor.list.col.max_count')}</span>
            <input
              type="number"
              className="mt-1 w-full border rounded p-2"
              value={draft.max_count ?? ''}
              onChange={e => setDraft({ ...draft, max_count: e.target.value ? Number(e.target.value) : undefined })}
            />
          </label>
          <div className="flex gap-2">
            <button className="btn-primary flex-1" onClick={onCreate}>{t('common.save')}</button>
            <button className="btn-ghost" onClick={() => setShowCreate(false)}>{t('common.cancel')}</button>
          </div>
        </div>
      </Modal>

      <Modal isOpen={!!created} onClose={() => setCreated(null)} title={t('auditor.list.created_title')}>
        {created && (
          <div className="space-y-3 text-sm">
            <div className="rounded bg-yellow-50 border border-yellow-200 p-3">
              <p className="font-semibold text-yellow-900">{t('auditor.list.temp_password_warning_title')}</p>
              <p className="text-yellow-800 mt-1">{t('auditor.list.temp_password_warning_body')}</p>
            </div>
            <div>
              <p className="text-gray-600">{t('auditor.list.temp_password')}</p>
              <div className="font-mono text-sm bg-gray-50 p-2 rounded border break-all">{created.temp_password}</div>
              <button className="btn-ghost mt-1" onClick={() => { navigator.clipboard.writeText(created.temp_password); alert(t('common.copied')); }}>
                <Copy className="w-3 h-3 inline mr-1" />{t('common.copy')}
              </button>
            </div>
            <div>
              <p className="text-gray-600">{t('auditor.list.invitation_link')}</p>
              <div className="font-mono text-xs bg-gray-50 p-2 rounded border break-all">{created.invitation_link}</div>
              <button className="btn-ghost mt-1" onClick={() => { navigator.clipboard.writeText(created.invitation_link); alert(t('common.copied')); }}>
                <Copy className="w-3 h-3 inline mr-1" />{t('common.copy')}
              </button>
            </div>
            <button className="btn-primary w-full" onClick={() => setCreated(null)}>{t('common.done')}</button>
          </div>
        )}
      </Modal>
    </div>
  );
}
