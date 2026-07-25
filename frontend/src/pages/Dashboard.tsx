import React, { useState, useEffect, ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { Wallet, ArrowLeftRight, CheckCircle, Clock, Inbox, Users, Eye } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Amount, Card, Hash, LoadingSpinner, PageHeader } from '../components/Common';
import {
  walletService,
  transferService,
  approvalService,
  payrollService,
  auditorAdminService,
} from '../services/api';
import { Wallet as WalletType, TransferListResponse } from '../types';
import { useAuth } from '../hooks/useAuth';

interface M1Stats {
  pending_approval_count: number;
  my_awaiting_count: number;
  employee_count: number;
  auditor_count: number;
}

export function Dashboard() {
  const { t } = useTranslation();
  const { user } = useAuth();
  const [wallets, setWallets] = useState<WalletType[]>([]);
  const [transfers, setTransfers] = useState<TransferListResponse | null>(null);
  const [m1Stats, setM1Stats] = useState<M1Stats>({
    pending_approval_count: 0,
    my_awaiting_count: 0,
    employee_count: 0,
    auditor_count: 0,
  });
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    loadData();
  }, []);

  const loadData = async () => {
    try {
      const [walletsData, transfersData] = await Promise.all([
        walletService.listWallets(),
        transferService.listTransfers(5, 0),
      ]);
      setWallets(walletsData);
      setTransfers(transfersData);

      // M1 stats — fire-and-forget; each independently caught so one 501
      // doesn't blank the whole dashboard while backend wire-up is incremental.
      const m1: M1Stats = { ...m1Stats };
      await Promise.allSettled([
        approvalService.listPending(1).then((r) => {
          m1.pending_approval_count = r.items.length;
        }),
        payrollService.listEmployees().then((e) => {
          m1.employee_count = e.length;
        }),
        user?.role === 'admin'
          ? auditorAdminService.list().then((a) => {
              m1.auditor_count = a.length;
            })
          : Promise.resolve(),
      ]);
      // my_awaiting_count: count of awaiting_approval transfers I created.
      // Done as a separate call once /transfers returns the extended status.
      try {
        const all = await transferService.listTransfers(50, 0);
        m1.my_awaiting_count = all.transfers.filter(
          (t) => (t.status as string) === 'awaiting_approval',
        ).length;
      } catch {
        /* no-op */
      }
      setM1Stats(m1);
    } catch (error) {
      console.error('Failed to load dashboard data:', error);
    } finally {
      setIsLoading(false);
    }
  };

  if (isLoading) return <LoadingSpinner size="lg" />;

  const confirmedTransfers =
    transfers?.transfers.filter((t) => t.status === 'confirmed').length || 0;

  const pendingTransfers =
    transfers?.transfers.filter((t) => t.status === 'pending' || t.status === 'submitted').length ||
    0;

  return (
    <>
      <PageHeader title={t('dashboard.title')} />

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Tile
          icon={<Wallet className="h-4 w-4 text-brand-600" />}
          tone="bg-brand-50"
          label={t('dashboard.totalWallets')}
          value={wallets.length}
        />
        <Tile
          icon={<ArrowLeftRight className="h-4 w-4 text-info-600" />}
          tone="bg-info-50"
          label={t('dashboard.totalTransfers')}
          value={transfers?.total || 0}
        />
        <Tile
          icon={<CheckCircle className="h-4 w-4 text-ok-600" />}
          tone="bg-ok-50"
          label={t('dashboard.confirmed')}
          value={confirmedTransfers}
        />
        <Tile
          icon={<Clock className="h-4 w-4 text-warn-600" />}
          tone="bg-warn-50"
          label={t('dashboard.pending')}
          value={pendingTransfers}
        />
      </div>

      {/* M1 stat cards — role-aware. Admins see all four; operators see
          two (my awaiting + employees); auditors don't see this row at all
          since their own dashboard is /auditor. */}
      {user?.role !== 'auditor' && (
        <div className="mt-3 grid grid-cols-2 gap-3 lg:grid-cols-4">
          {user?.role === 'admin' && (
            <Tile
              to="/approval/queue"
              icon={<Inbox className="h-4 w-4 text-warn-600" />}
              tone="bg-warn-50"
              label={t('dashboard.m1.pending_approvals')}
              value={m1Stats.pending_approval_count}
            />
          )}
          <Tile
            to="/approval/pending"
            icon={<Clock className="h-4 w-4 text-legacy-600" />}
            tone="bg-legacy-50"
            label={t('dashboard.m1.my_awaiting')}
            value={m1Stats.my_awaiting_count}
          />
          <Tile
            to="/payroll/employees"
            icon={<Users className="h-4 w-4 text-iron-600" />}
            tone="bg-iron-50"
            label={t('dashboard.m1.employees')}
            value={m1Stats.employee_count}
          />
          {user?.role === 'admin' && (
            <Tile
              to="/auditor/manage"
              icon={<Eye className="h-4 w-4 text-brand-600" />}
              tone="bg-brand-50"
              label={t('dashboard.m1.auditors')}
              value={m1Stats.auditor_count}
            />
          )}
        </div>
      )}

      <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card title={t('dashboard.activeWallets')} flush>
          {wallets.length === 0 ? (
            <p className="px-4 py-8 text-center text-[0.8125rem] text-ink-400">
              {t('dashboard.noWallets')}
            </p>
          ) : (
            <ul className="divide-y divide-line-100">
              {wallets.slice(0, 5).map((wallet) => (
                <li key={wallet.id} className="flex items-center justify-between gap-3 px-4 py-3">
                  <div className="min-w-0">
                    <p className="truncate text-[0.8125rem] font-medium text-ink-900">
                      {wallet.name}
                    </p>
                    <Hash value={wallet.address} head={12} tail={8} />
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <span className="badge badge-neutral uppercase">{wallet.chain}</span>
                    {wallet.is_active && <span className="badge badge-ok">{t('common.active')}</span>}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </Card>

        <Card title={t('dashboard.recentTransfers')} flush>
          {!transfers?.transfers.length ? (
            <p className="px-4 py-8 text-center text-[0.8125rem] text-ink-400">
              {t('dashboard.noTransfers')}
            </p>
          ) : (
            <ul className="divide-y divide-line-100">
              {transfers.transfers.map((transfer) => (
                <li key={transfer.id} className="flex items-center justify-between gap-3 px-4 py-3">
                  <div className="min-w-0">
                    <p className="text-[0.8125rem] text-ink-900">
                      <Amount value={transfer.amount} unit={transfer.token} />
                    </p>
                    <div className="flex items-center gap-1 text-[0.6875rem] text-ink-400">
                      <span>{t('dashboard.to')}</span>
                      <Hash value={transfer.to_address} head={10} tail={6} />
                    </div>
                  </div>
                  <span
                    className={`badge shrink-0 ${
                      transfer.status === 'confirmed'
                        ? 'badge-ok'
                        : transfer.status === 'failed'
                          ? 'badge-bad'
                          : 'badge-warn'
                    }`}
                  >
                    {t(`status.${transfer.status}`)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>
    </>
  );
}

/** Compact metric tile; becomes a link when the number has a page behind it. */
function Tile({
  icon,
  tone,
  label,
  value,
  to,
}: {
  icon: ReactNode;
  tone: string;
  label: string;
  value: ReactNode;
  to?: string;
}) {
  const body = (
    <div className="card flex items-center gap-3 px-4 py-3.5 transition-colors">
      <span className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px] ${tone}`}>
        {icon}
      </span>
      <div className="min-w-0">
        <p className="truncate text-[0.6875rem] font-medium uppercase tracking-[0.04em] text-ink-400">
          {label}
        </p>
        <p className="num mt-0.5 text-xl font-semibold leading-none text-ink-900">{value}</p>
      </div>
    </div>
  );
  return to ? (
    <Link to={to} className="block [&>div:hover]:border-brand-200">
      {body}
    </Link>
  ) : (
    body
  );
}
