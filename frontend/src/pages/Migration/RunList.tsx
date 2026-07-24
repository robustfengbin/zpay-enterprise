import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';
import { Plus, RefreshCw } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { migrationService } from '../../services/api/migration';
import type { MigrationRun } from '../../types/migration';

/** F4.1 — Migration run list. */
export function MigrationRunList() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [runs, setRuns] = useState<MigrationRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    try {
      setRuns(await migrationService.listRuns());
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void load(); }, []);

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('migration.list.title')}</h1>
        <div className="flex gap-2">
          <button className="btn-ghost" onClick={() => void load()} aria-label={t('common.refresh')}>
            <RefreshCw className="w-4 h-4" />
          </button>
          <button className="btn-primary" onClick={() => navigate('/migrations/new')}>
            <Plus className="w-4 h-4 inline mr-1" /> {t('migration.list.new')}
          </button>
        </div>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-gray-500 uppercase text-xs">
            <tr>
              <th className="text-left p-2">#</th>
              <th className="text-left p-2">{t('migration.list.col.wallet')}</th>
              <th className="text-left p-2">{t('migration.list.col.mode')}</th>
              <th className="text-right p-2">{t('migration.list.col.total')}</th>
              <th className="text-left p-2">{t('migration.list.col.status')}</th>
              <th className="text-left p-2">{t('migration.list.col.created')}</th>
            </tr>
          </thead>
          <tbody>
            {runs.map(run => (
              <tr
                key={run.id}
                className="border-t border-gray-100 hover:bg-gray-50 cursor-pointer"
                onClick={() => navigate(`/migrations/${run.id}`)}
              >
                <td className="p-2 font-mono">
                  <Link to={`/migrations/${run.id}`} className="text-blue-600">#{run.id}</Link>
                </td>
                <td className="p-2">#{run.source_wallet_id}</td>
                <td className="p-2">{t(`migration.create.mode_${run.mode}`)}</td>
                <td className="p-2 text-right font-mono">{Number(run.total_amount)}</td>
                <td className="p-2">
                  <span className="px-2 py-0.5 rounded bg-gray-100 text-gray-800 text-xs">
                    {t(`migration.run_status.${run.status}`)}
                  </span>
                </td>
                <td className="p-2 text-gray-500">{new Date(run.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {runs.length === 0 && (
              <tr><td colSpan={6} className="p-6 text-center text-gray-400">{t('migration.list.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}
