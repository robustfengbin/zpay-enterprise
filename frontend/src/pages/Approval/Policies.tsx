import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Trash2 } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { approvalService } from '../../services/api';
import type { ApprovalPolicy, CreatePolicyRequest, PolicyScope } from '../../types/approval';

/**
 * Admin-only: Approval policy management. Reachable via Settings.
 * Per PRD-F2.1 §5.6.
 */
export function ApprovalPolicies() {
  const { t } = useTranslation();
  const [policies, setPolicies] = useState<ApprovalPolicy[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [draft, setDraft] = useState<CreatePolicyRequest>({
    scope: 'global',
    chain: 'ethereum',
    token: 'USDT',
    amount_threshold: '5000',
    sla_minutes: 1440,
    required_count: 1,
  });

  async function load() {
    setLoading(true);
    try {
      setPolicies(await approvalService.listPolicies());
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => { load(); }, []);

  async function onCreate() {
    try {
      await approvalService.createPolicy(draft);
      setShowForm(false);
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  async function onDelete(id: number) {
    if (!confirm(t('approval.policies.confirm_delete'))) return;
    try {
      await approvalService.deletePolicy(id);
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 space-y-4 max-w-4xl">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('approval.policies.title')}</h1>
        <button className="btn-primary" onClick={() => setShowForm(true)}>
          <Plus className="w-4 h-4 inline mr-1" />{t('approval.policies.new')}
        </button>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-xs text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">{t('approval.policies.col.scope')}</th>
              <th className="text-left p-2">{t('approval.policies.col.chain')}</th>
              <th className="text-left p-2">{t('approval.policies.col.token')}</th>
              <th className="text-left p-2">{t('approval.policies.col.threshold')}</th>
              <th className="text-left p-2">{t('approval.policies.col.sla')}</th>
              <th className="text-left p-2">{t('approval.policies.col.required_count')}</th>
              <th className="text-right p-2">{t('common.action')}</th>
            </tr>
          </thead>
          <tbody>
            {policies.map(p => (
              <tr key={p.id} className="border-t border-gray-100">
                <td className="p-2">{p.scope}{p.scope_id && ` (#${p.scope_id})`}</td>
                <td className="p-2">{p.chain}</td>
                <td className="p-2 font-medium">{p.token}</td>
                <td className="p-2 font-mono">{p.amount_threshold}</td>
                <td className="p-2">{p.sla_minutes} min</td>
                <td className="p-2">{p.required_count}</td>
                <td className="p-2 text-right">
                  <button className="btn-ghost" onClick={() => onDelete(p.id)} title={t('common.delete')}>
                    <Trash2 className="w-4 h-4 text-red-600" />
                  </button>
                </td>
              </tr>
            ))}
            {policies.length === 0 && (
              <tr><td colSpan={7} className="p-4 text-center text-gray-400">{t('approval.policies.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>

      {showForm && (
        <Card>
          <h2 className="font-semibold mb-3">{t('approval.policies.new')}</h2>
          <div className="grid grid-cols-2 gap-3 text-sm">
            <label>
              <span className="text-gray-600">{t('approval.policies.col.scope')}</span>
              <select
                className="mt-1 w-full border rounded p-2"
                value={draft.scope}
                onChange={e => setDraft({ ...draft, scope: e.target.value as PolicyScope })}
              >
                <option value="global">global</option>
                <option value="wallet">wallet</option>
                <option value="user">user</option>
              </select>
            </label>
            <label>
              <span className="text-gray-600">{t('approval.policies.col.scope_id_optional')}</span>
              <input
                type="number"
                className="mt-1 w-full border rounded p-2"
                value={draft.scope_id ?? ''}
                onChange={e => setDraft({ ...draft, scope_id: e.target.value ? Number(e.target.value) : undefined })}
                disabled={draft.scope === 'global'}
              />
            </label>
            <label>
              <span className="text-gray-600">{t('approval.policies.col.chain')}</span>
              <input className="mt-1 w-full border rounded p-2" value={draft.chain} onChange={e => setDraft({ ...draft, chain: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('approval.policies.col.token')}</span>
              <input className="mt-1 w-full border rounded p-2" value={draft.token} onChange={e => setDraft({ ...draft, token: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('approval.policies.col.threshold')}</span>
              <input className="mt-1 w-full border rounded p-2" value={draft.amount_threshold} onChange={e => setDraft({ ...draft, amount_threshold: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('approval.policies.col.sla')}</span>
              <input
                type="number"
                className="mt-1 w-full border rounded p-2"
                value={draft.sla_minutes}
                onChange={e => setDraft({ ...draft, sla_minutes: Number(e.target.value) })}
              />
            </label>
          </div>
          <div className="mt-4 flex gap-3">
            <button className="btn-primary" onClick={onCreate}>{t('common.save')}</button>
            <button className="btn-ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</button>
          </div>
        </Card>
      )}
    </div>
  );
}
