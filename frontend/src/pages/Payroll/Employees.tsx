import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Trash2, Edit2, Save, X } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { payrollService } from '../../services/api';
import type { Employee } from '../../types/payroll';

/**
 * F3.2 — Employee profile CRUD (M1 view: 5 columns).
 *
 * Aligned with backend Employee model in PRD-F3.1 §4.1:
 *   { employee_code, name, wallet_address, chain, tags JSON, active }
 *
 * tags JSON intentionally hidden in the M1 UI — M2+ unlocks per-employee
 * preferred_token / privacy_mode / kyc_status when multi-currency support
 * lands (currently frozen).
 */
export function PayrollEmployees() {
  const { t } = useTranslation();
  const [list, setList] = useState<Employee[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Employee | null>(null);
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState<Omit<Employee, 'id' | 'created_at' | 'updated_at'>>({
    employee_code: '',
    name: '',
    wallet_address: '',
    chain: 'zcash',
    tags: null,
    active: true,
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
      setDraft({ employee_code: '', name: '', wallet_address: '', chain: 'zcash', tags: null, active: true });
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
              <th className="text-left p-2">{t('payroll.employees.col.code')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.name')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.address')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.chain')}</th>
              <th className="text-left p-2">{t('payroll.employees.col.active')}</th>
              <th className="text-right p-2">{t('common.action')}</th>
            </tr>
          </thead>
          <tbody>
            {list.map(emp => editing && editing.id === emp.id ? (
              <tr key={emp.id} className="border-t border-blue-200 bg-blue-50">
                <td className="p-2">
                  <input className="w-full border rounded p-1 font-mono text-xs" value={editing.employee_code} onChange={e => setEditing({ ...editing, employee_code: e.target.value })} />
                </td>
                <td className="p-2">
                  <input className="w-full border rounded p-1" value={editing.name} onChange={e => setEditing({ ...editing, name: e.target.value })} />
                </td>
                <td className="p-2">
                  <input className="w-full border rounded p-1 font-mono text-xs" value={editing.wallet_address} onChange={e => setEditing({ ...editing, wallet_address: e.target.value })} />
                </td>
                <td className="p-2">
                  <select className="w-full border rounded p-1" value={editing.chain} onChange={e => setEditing({ ...editing, chain: e.target.value })}>
                    <option value="zcash">zcash</option>
                    <option value="ethereum">ethereum</option>
                  </select>
                </td>
                <td className="p-2">
                  <input type="checkbox" checked={editing.active} onChange={e => setEditing({ ...editing, active: e.target.checked })} />
                </td>
                <td className="p-2 text-right space-x-1">
                  <button className="btn-ghost" onClick={onSaveEdit} title={t('common.save')}><Save className="w-4 h-4 text-green-600" /></button>
                  <button className="btn-ghost" onClick={() => setEditing(null)} title={t('common.cancel')}><X className="w-4 h-4" /></button>
                </td>
              </tr>
            ) : (
              <tr key={emp.id} className={`border-t border-gray-100 ${!emp.active ? 'opacity-60' : ''}`}>
                <td className="p-2 font-mono text-xs">{emp.employee_code}</td>
                <td className="p-2">{emp.name}</td>
                <td className="p-2 font-mono text-xs truncate max-w-[200px]">{emp.wallet_address}</td>
                <td className="p-2">{emp.chain}</td>
                <td className="p-2">{emp.active ? '✓' : '—'}</td>
                <td className="p-2 text-right space-x-1">
                  <button className="btn-ghost" onClick={() => setEditing(emp)} title={t('common.edit')}><Edit2 className="w-4 h-4" /></button>
                  <button className="btn-ghost" onClick={() => onDelete(emp.id)} title={t('common.delete')}><Trash2 className="w-4 h-4 text-red-600" /></button>
                </td>
              </tr>
            ))}
            {list.length === 0 && (
              <tr><td colSpan={6} className="p-4 text-center text-gray-400">{t('payroll.employees.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>

      {creating && (
        <Card>
          <h2 className="font-semibold mb-3">{t('payroll.employees.new')}</h2>
          <div className="grid grid-cols-2 gap-3 text-sm">
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.code')}</span>
              <input className="mt-1 w-full border rounded p-2 font-mono text-xs" value={draft.employee_code} onChange={e => setDraft({ ...draft, employee_code: e.target.value })} placeholder="EMP-001" />
            </label>
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.name')}</span>
              <input className="mt-1 w-full border rounded p-2" value={draft.name} onChange={e => setDraft({ ...draft, name: e.target.value })} />
            </label>
            <label className="col-span-2">
              <span className="text-gray-600">{t('payroll.employees.col.address')}</span>
              <input className="mt-1 w-full border rounded p-2 font-mono text-xs" value={draft.wallet_address} onChange={e => setDraft({ ...draft, wallet_address: e.target.value })} />
            </label>
            <label>
              <span className="text-gray-600">{t('payroll.employees.col.chain')}</span>
              <select className="mt-1 w-full border rounded p-2" value={draft.chain} onChange={e => setDraft({ ...draft, chain: e.target.value })}>
                <option value="zcash">zcash</option>
                <option value="ethereum">ethereum</option>
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
