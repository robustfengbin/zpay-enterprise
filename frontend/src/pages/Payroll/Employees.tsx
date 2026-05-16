import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Trash2, Edit2, Save, X } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { payrollService } from '../../services/api';
import type { Employee, PayrollPrivacyMode } from '../../types/payroll';

/**
 * F3.2 — Employee profile management (CRUD).
 * Listed as a separate page so Payroll Run create can pick from existing
 * employees instead of always re-keying CSV.
 */
export function PayrollEmployees() {
  const { t } = useTranslation();
  const [list, setList] = useState<Employee[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Employee | null>(null);
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState<Omit<Employee, 'id' | 'created_at'>>({
    name: '',
    wallet_address: '',
    preferred_chain: 'zcash',
    preferred_token: 'ZEC',
    privacy_mode: 'shielded',
    kyc_status: 'pending',
  });

  async function load() {
    setLoading(true);
    try {
      setList(await payrollService.listEmployees());
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => { load(); }, []);

  async function onCreate() {
    try {
      await payrollService.createEmployee(draft);
      setCreating(false);
      setDraft({ name: '', wallet_address: '', preferred_chain: 'zcash', preferred_token: 'ZEC', privacy_mode: 'shielded', kyc_status: 'pending' });
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  async function onSaveEdit() {
    if (!editing) return;
    try {
      await payrollService.updateEmployee(editing.id, editing);
      setEditing(null);
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  async function onDelete(id: number) {
    if (!confirm(t('payroll.employees.confirm_delete'))) return;
    try {
      await payrollService.deleteEmployee(id);
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('payroll.employees.title')}</h1>
        <button className="btn-primary" onClick={() => setCreating(true)}>
          <Plus className="w-4 h-4 inline mr-1" /> {t('payroll.employees.new')}
        </button>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-xs text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">{t('payroll.employees.col.name')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.address')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.chain')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.token')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.privacy')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.kyc')}</th>
              <th className="text-right p-2">{t('common.action')}</th>
            </tr>
          </thead>
          <tbody>
            {list.map(emp => editing && editing.id === emp.id ? (
              <tr key={emp.id} className="border-t border-blue-200 bg-blue-50">
                <td className="p-2">
                  <input className="w-full border rounded p-1" value={editing.name} onChange={e => setEditing({ ...editing, name: e.target.value })} />
                </td>
                <td className="p-2">
                  <input className="w-full border rounded p-1 font-mono text-xs" value={editing.wallet_address} onChange={e => setEditing({ ...editing, wallet_address: e.target.value })} />
                </td>
                <td className="p-2">
                  <input className="w-full border rounded p-1" value={editing.preferred_chain} onChange={e => setEditing({ ...editing, preferred_chain: e.target.value })} />
                </td>
                <td className="p-2">
                  <input className="w-full border rounded p-1" value={editing.preferred_token} onChange={e => setEditing({ ...editing, preferred_token: e.target.value })} />
                </td>
                <td className="p-2">
                  <select
                    className="w-full border rounded p-1"
                    value={editing.privacy_mode}
                    onChange={e => setEditing({ ...editing, privacy_mode: e.target.value as PayrollPrivacyMode })}
                  >
                    <option value="shielded">shielded</option>
                    <option value="direct">direct</option>
                  </select>
                </td>
                <td className="p-2">{emp.kyc_status}</td>
                <td className="p-2 text-right space-x-1">
                  <button className="btn-ghost" onClick={onSaveEdit} title={t('common.save')}><Save className="w-4 h-4 text-green-600" /></button>
                  <button className="btn-ghost" onClick={() => setEditing(null)} title={t('common.cancel')}><X className="w-4 h-4" /></button>
                </td>
              </tr>
            ) : (
              <tr key={emp.id} className="border-t border-gray-100">
                <td className="p-2">{emp.name}</td>
                <td className="p-2 font-mono text-xs truncate max-w-[200px]">{emp.wallet_address}</td>
                <td className="p-2">{emp.preferred_chain}</td>
                <td className="p-2">{emp.preferred_token}</td>
                <td className="p-2">{emp.privacy_mode}</td>
                <td className="p-2">{emp.kyc_status}</td>
                <td className="p-2 text-right space-x-1">
                  <button className="btn-ghost" onClick={() => setEditing(emp)} title={t('common.edit')}><Edit2 className="w-4 h-4" /></button>
                  <button className="btn-ghost" onClick={() => onDelete(emp.id)} title={t('common.delete')}><Trash2 className="w-4 h-4 text-red-600" /></button>
                </td>
              </tr>
            ))}
            {list.length === 0 && (
              <tr><td colSpan={7} className="p-4 text-center text-gray-400">{t('payroll.employees.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>

      {creating && (
        <Card>
          <h2 className="font-semibold mb-3">{t('payroll.employees.new')}</h2>
          <div className="grid grid-cols-2 gap-3 text-sm">
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.name')}</span>
              <input className="mt-1 w-full border rounded p-2" value={draft.name} onChange={e => setDraft({ ...draft, name: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.address')}</span>
              <input className="mt-1 w-full border rounded p-2 font-mono text-xs" value={draft.wallet_address} onChange={e => setDraft({ ...draft, wallet_address: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.chain')}</span>
              <input className="mt-1 w-full border rounded p-2" value={draft.preferred_chain} onChange={e => setDraft({ ...draft, preferred_chain: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.token')}</span>
              <input className="mt-1 w-full border rounded p-2" value={draft.preferred_token} onChange={e => setDraft({ ...draft, preferred_token: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.privacy')}</span>
              <select
                className="mt-1 w-full border rounded p-2"
                value={draft.privacy_mode}
                onChange={e => setDraft({ ...draft, privacy_mode: e.target.value as PayrollPrivacyMode })}
              >
                <option value="shielded">shielded</option>
                <option value="direct">direct</option>
              </select>
            </label>
          </div>
          <div className="mt-4 flex gap-3">
            <button className="btn-primary" onClick={onCreate}>{t('common.save')}</button>
            <button className="btn-ghost" onClick={() => setCreating(false)}>{t('common.cancel')}</button>
          </div>
        </Card>
      )}
    </div>
  );
}
