import React, { ReactNode } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import {
  LayoutDashboard,
  Wallet,
  ArrowLeftRight,
  History,
  Settings,
  Server,
  ChevronDown,
  ChevronRight,
  CheckSquare,
  Inbox,
  Users,
  FileText,
  Send,
  ShieldCheck,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '../../hooks/useAuth';

const CHAIN_LIST = [
  { id: 'ethereum', name: 'Ethereum', icon: '⟠', color: '#627EEA' },
  { id: 'zcash', name: 'Zcash', icon: 'Ⓩ', color: '#F4B728' },
];

const ITEM =
  'group relative flex items-center gap-2.5 rounded-lg px-2.5 py-[7px] text-[0.8125rem] transition-colors';
const ITEM_IDLE = 'text-[#96a0b5] hover:bg-white/[0.06] hover:text-white';
const ITEM_ACTIVE = 'bg-white/[0.09] text-white font-medium';

/** Active items carry a brand bar on the rail edge — a marker, not a slab. */
function ActiveBar() {
  return (
    <span className="absolute -left-2.5 top-1/2 h-4 w-[3px] -translate-y-1/2 rounded-r bg-brand-400" />
  );
}

function Item({
  to,
  icon: Icon,
  label,
  end,
  indent,
}: {
  to: string;
  icon: LucideIcon;
  label: string;
  end?: boolean;
  indent?: boolean;
}) {
  return (
    <NavLink
      to={to}
      end={end}
      className={({ isActive }) =>
        `${ITEM} ${isActive ? ITEM_ACTIVE : ITEM_IDLE} ${indent ? 'ml-5' : ''}`
      }
    >
      {({ isActive }) => (
        <>
          {isActive && <ActiveBar />}
          <Icon className={`shrink-0 ${indent ? 'h-3.5 w-3.5' : 'h-4 w-4'}`} />
          <span className="truncate">{label}</span>
        </>
      )}
    </NavLink>
  );
}

function Group({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="mt-5 first:mt-0">
      <p className="px-2.5 pb-1.5 text-[0.625rem] font-semibold uppercase tracking-[0.09em] text-[#5f6b80]">
        {label}
      </p>
      <div className="space-y-0.5">{children}</div>
    </div>
  );
}

function ChainSection({
  chain,
  isExpanded,
}: {
  chain: (typeof CHAIN_LIST)[number];
  isExpanded: boolean;
}) {
  const { t } = useTranslation();
  const base = `/${chain.id}`;

  return (
    <div>
      <NavLink
        to={`${base}/wallets`}
        className={`${ITEM} ${isExpanded ? 'text-white' : ITEM_IDLE} justify-between`}
      >
        <span className="flex items-center gap-2.5">
          <span
            className="flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-full text-[10px] text-white"
            style={{ backgroundColor: chain.color }}
          >
            {chain.icon}
          </span>
          {chain.name}
        </span>
        {isExpanded ? (
          <ChevronDown className="h-3.5 w-3.5 opacity-60" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 opacity-60" />
        )}
      </NavLink>

      {isExpanded && (
        <div className="relative mt-0.5 space-y-0.5 before:absolute before:bottom-1 before:left-[13px] before:top-1 before:w-px before:bg-white/10">
          <Item to={`${base}/wallets`} icon={Wallet} label={t('sidebar.wallets')} indent />
          <Item to={`${base}/rpc`} icon={Server} label={t('sidebar.rpcSettings')} indent />
        </div>
      )}
    </div>
  );
}

export function Sidebar() {
  const { t } = useTranslation();
  const { user } = useAuth();
  const location = useLocation();
  const isAdmin = user?.role === 'admin';
  const isOperator = user?.role === 'operator';

  return (
    <aside className="flex h-screen w-[248px] min-w-[248px] shrink-0 flex-col bg-rail-900 text-white">
      <div className="flex items-center gap-2.5 px-5 py-4">
        <span className="flex h-8 w-8 items-center justify-center rounded-[9px] bg-brand-600 text-[15px] font-bold shadow-[0_2px_8px_rgba(75,86,221,0.45)]">
          z
        </span>
        <div className="leading-tight">
          <p className="text-[0.9375rem] font-semibold tracking-[-0.01em]">{t('sidebar.title')}</p>
          <p className="text-[0.6875rem] text-[#7b8699]">{t('sidebar.subtitle')}</p>
        </div>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto px-3.5 pb-4">
        <div className="space-y-0.5">
          <Item to="/" end icon={LayoutDashboard} label={t('sidebar.dashboard')} />
        </div>

        <Group label={t('sidebar.chains')}>
          {CHAIN_LIST.map((chain) => (
            <ChainSection
              key={chain.id}
              chain={chain}
              isExpanded={location.pathname.startsWith(`/${chain.id}`)}
            />
          ))}
        </Group>

        {/* Every way money leaves a wallet, in one group: single transfers,
            batch payouts, and the Ironwood migration. */}
        <Group label={t('sidebar.transfers_group')}>
          <Item to="/ethereum/transfer" icon={ArrowLeftRight} label={t('sidebar.transfer_eth')} />
          <Item to="/zcash/transfer" icon={ArrowLeftRight} label={t('sidebar.transfer_zec')} />
          {isAdmin && (
            <Item to="/batch-transfers" icon={Send} label={t('sidebar.batch_transfers')} />
          )}
          {isAdmin && (
            <Item to="/migrations" icon={ShieldCheck} label={t('sidebar.migrations')} />
          )}
        </Group>

        <Group label={t('sidebar.governance')}>
          {isAdmin && <Item to="/approval/queue" icon={Inbox} label={t('sidebar.approval_queue')} />}
          {(isAdmin || isOperator) && (
            <Item to="/approval/pending" icon={CheckSquare} label={t('sidebar.my_approvals')} />
          )}
          {isAdmin && (
            <Item to="/approval/policies" icon={Settings} label={t('sidebar.approval_policies')} />
          )}
          {/* The /auditor dashboard is the auditor's own read-only view (gated
              by AuditorAuthMiddleware on kind="auditor" JWTs), so admins manage
              auditors at /auditor/manage instead of linking there. */}
          {isAdmin && <Item to="/auditor/manage" icon={Users} label={t('sidebar.auditor_manage')} />}
        </Group>

        <Group label={t('sidebar.payroll')}>
          <Item to="/payroll/runs" icon={FileText} label={t('sidebar.payroll_runs')} />
          <Item to="/payroll/employees" icon={Users} label={t('sidebar.employees')} />
        </Group>

        <Group label={t('sidebar.general')}>
          <Item to="/history" icon={History} label={t('sidebar.history')} />
          <Item to="/settings" icon={Settings} label={t('sidebar.settings')} />
        </Group>
      </nav>
    </aside>
  );
}
