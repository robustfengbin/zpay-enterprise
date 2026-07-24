import React from 'react';
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
  Eye,
  Users,
  FileText,
  Send,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '../../hooks/useAuth';

// Chain configuration
const CHAIN_LIST = [
  { id: 'ethereum', name: 'Ethereum', icon: '⟠', color: '#627EEA' },
  { id: 'zcash', name: 'Zcash', icon: 'Ⓩ', color: '#F4B728' },
];

interface ChainSectionProps {
  chain: { id: string; name: string; icon: string; color: string };
  isExpanded: boolean;
}

function ChainSection({ chain, isExpanded }: ChainSectionProps) {
  const { t } = useTranslation();
  const basePath = `/${chain.id}`;

  return (
    <div>
      <NavLink
        to={`${basePath}/wallets`}
        className={({ isActive }) =>
          `flex items-center justify-between px-6 py-3 text-sm transition-colors ${
            isActive || isExpanded
              ? 'bg-gray-800 text-white'
              : 'text-gray-300 hover:bg-gray-800 hover:text-white'
          }`
        }
      >
        <div className="flex items-center">
          <span
            className="w-6 h-6 rounded-full flex items-center justify-center text-white text-xs mr-3"
            style={{ backgroundColor: chain.color }}
          >
            {chain.icon}
          </span>
          <span>{chain.name}</span>
        </div>
        {isExpanded ? (
          <ChevronDown className="w-4 h-4" />
        ) : (
          <ChevronRight className="w-4 h-4" />
        )}
      </NavLink>

      {isExpanded && (
        <div className="bg-gray-950">
          <NavLink
            to={`${basePath}/wallets`}
            className={({ isActive }) =>
              `flex items-center px-6 pl-12 py-2 text-sm transition-colors ${
                isActive
                  ? 'text-white bg-blue-600'
                  : 'text-gray-400 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <Wallet className="w-4 h-4 mr-3 flex-shrink-0" />
            <span>{t('sidebar.wallets')}</span>
          </NavLink>
          <NavLink
            to={`${basePath}/rpc`}
            className={({ isActive }) =>
              `flex items-center px-6 pl-12 py-2 text-sm transition-colors ${
                isActive
                  ? 'text-white bg-blue-600'
                  : 'text-gray-400 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <Server className="w-4 h-4 mr-3 flex-shrink-0" />
            <span>{t('sidebar.rpcSettings')}</span>
          </NavLink>
        </div>
      )}
    </div>
  );
}

export function Sidebar() {
  const { t } = useTranslation();
  const { user } = useAuth();
  const location = useLocation();

  return (
    <aside className="w-64 min-w-64 bg-gray-900 text-white h-screen shrink-0" style={{ display: 'flex', flexDirection: 'column' }}>
      {/* Header */}
      <div className="p-6 shrink-0">
        <h1 className="text-xl font-bold">{t('sidebar.title')}</h1>
        <p className="text-gray-400 text-sm mt-1">{t('sidebar.subtitle')}</p>
      </div>

      {/* Navigation */}
      <nav className="flex-1 overflow-y-auto" style={{ minHeight: 0 }}>
        {/* Dashboard */}
        <NavLink
          to="/"
          end
          className={({ isActive }) =>
            `flex items-center px-6 py-3 text-sm transition-colors ${
              isActive
                ? 'bg-blue-600 text-white'
                : 'text-gray-300 hover:bg-gray-800 hover:text-white'
            }`
          }
        >
          <LayoutDashboard className="w-5 h-5 mr-3 flex-shrink-0" />
          <span>{t('sidebar.dashboard')}</span>
        </NavLink>

        {/* Chain Sections */}
        <div className="mt-4">
          <div className="px-6 py-2 text-xs font-semibold text-gray-500 uppercase tracking-wider">
            {t('sidebar.chains')}
          </div>

          {CHAIN_LIST.map((chain) => (
            <ChainSection
              key={chain.id}
              chain={chain}
              isExpanded={location.pathname.startsWith(`/${chain.id}`)}
            />
          ))}
        </div>

        {/* Transfers — every way money leaves a wallet, in one group:
            single (per-chain), batch payouts, and the Ironwood migration. */}
        <div className="mt-4">
          <div className="px-6 py-2 text-xs font-semibold text-gray-500 uppercase tracking-wider">
            {t('sidebar.transfers_group')}
          </div>
          <NavLink
            to="/ethereum/transfer"
            className={({ isActive }) =>
              `flex items-center px-6 py-3 text-sm transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white'
                  : 'text-gray-300 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <ArrowLeftRight className="w-5 h-5 mr-3 flex-shrink-0" />
            <span>{t('sidebar.transfer_eth')}</span>
          </NavLink>
          <NavLink
            to="/zcash/transfer"
            className={({ isActive }) =>
              `flex items-center px-6 py-3 text-sm transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white'
                  : 'text-gray-300 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <ArrowLeftRight className="w-5 h-5 mr-3 flex-shrink-0" />
            <span>{t('sidebar.transfer_zec')}</span>
          </NavLink>
          {/* F4.2 — batch privacy transfers (admin-only, treasury payouts) */}
          {user?.role === 'admin' && (
            <NavLink
              to="/batch-transfers"
              className={({ isActive }) =>
                `flex items-center px-6 py-3 text-sm transition-colors ${
                  isActive
                    ? 'bg-blue-600 text-white'
                    : 'text-gray-300 hover:bg-gray-800 hover:text-white'
                }`
              }
            >
              <Send className="w-5 h-5 mr-3 flex-shrink-0" />
              <span>{t('sidebar.batch_transfers')}</span>
            </NavLink>
          )}
          {/* F4.1 — Ironwood migration runs (admin-only, whole-treasury moves) */}
          {user?.role === 'admin' && (
            <NavLink
              to="/migrations"
              className={({ isActive }) =>
                `flex items-center px-6 py-3 text-sm transition-colors ${
                  isActive
                    ? 'bg-blue-600 text-white'
                    : 'text-gray-300 hover:bg-gray-800 hover:text-white'
                }`
              }
            >
              <FileText className="w-5 h-5 mr-3 flex-shrink-0" />
              <span>{t('sidebar.migrations')}</span>
            </NavLink>
          )}
        </div>

        {/* M1 Governance & Compliance */}
        <div className="mt-4">
          <div className="px-6 py-2 text-xs font-semibold text-gray-500 uppercase tracking-wider">
            {t('sidebar.governance')}
          </div>
          {/* Approval — visible to admin (checker) + operator (maker views own pending) */}
          {(user?.role === 'admin' || user?.role === 'operator') && (
            <>
              {user?.role === 'admin' && (
                <NavLink
                  to="/approval/queue"
                  className={({ isActive }) =>
                    `flex items-center px-6 py-3 text-sm transition-colors ${
                      isActive
                        ? 'bg-blue-600 text-white'
                        : 'text-gray-300 hover:bg-gray-800 hover:text-white'
                    }`
                  }
                >
                  <Inbox className="w-5 h-5 mr-3 flex-shrink-0" />
                  <span>{t('sidebar.approval_queue')}</span>
                </NavLink>
              )}
              <NavLink
                to="/approval/pending"
                className={({ isActive }) =>
                  `flex items-center px-6 py-3 text-sm transition-colors ${
                    isActive
                      ? 'bg-blue-600 text-white'
                      : 'text-gray-300 hover:bg-gray-800 hover:text-white'
                  }`
                }
              >
                <CheckSquare className="w-5 h-5 mr-3 flex-shrink-0" />
                <span>{t('sidebar.my_approvals')}</span>
              </NavLink>
              {user?.role === 'admin' && (
                <NavLink
                  to="/approval/policies"
                  className={({ isActive }) =>
                    `flex items-center px-6 py-3 text-sm transition-colors ${
                      isActive
                        ? 'bg-blue-600 text-white'
                        : 'text-gray-300 hover:bg-gray-800 hover:text-white'
                    }`
                  }
                >
                  <Settings className="w-5 h-5 mr-3 flex-shrink-0" />
                  <span>{t('sidebar.approval_policies')}</span>
                </NavLink>
              )}
            </>
          )}

          {/* Auditor management. The /auditor dashboard is the auditor's own
              read-only view (gated by AuditorAuthMiddleware on kind="auditor"
              JWT), so admins shouldn't link there from the sidebar — they
              manage auditors at /auditor/manage instead. */}
          {user?.role === 'admin' && (
            <NavLink
              to="/auditor/manage"
              className={({ isActive }) =>
                `flex items-center px-6 py-3 text-sm transition-colors ${
                  isActive
                    ? 'bg-blue-600 text-white'
                    : 'text-gray-300 hover:bg-gray-800 hover:text-white'
                }`
              }
            >
              <Users className="w-5 h-5 mr-3 flex-shrink-0" />
              <span>{t('sidebar.auditor_manage')}</span>
            </NavLink>
          )}
        </div>

        {/* M1 Payroll */}
        <div className="mt-4">
          <div className="px-6 py-2 text-xs font-semibold text-gray-500 uppercase tracking-wider">
            {t('sidebar.payroll')}
          </div>
          <NavLink
            to="/payroll/runs"
            className={({ isActive }) =>
              `flex items-center px-6 py-3 text-sm transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white'
                  : 'text-gray-300 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <FileText className="w-5 h-5 mr-3 flex-shrink-0" />
            <span>{t('sidebar.payroll_runs')}</span>
          </NavLink>
          <NavLink
            to="/payroll/employees"
            className={({ isActive }) =>
              `flex items-center px-6 py-3 text-sm transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white'
                  : 'text-gray-300 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <Users className="w-5 h-5 mr-3 flex-shrink-0" />
            <span>{t('sidebar.employees')}</span>
          </NavLink>
        </div>

        {/* History & Settings */}
        <div className="mt-4">
          <div className="px-6 py-2 text-xs font-semibold text-gray-500 uppercase tracking-wider">
            {t('sidebar.general')}
          </div>
          <NavLink
            to="/history"
            className={({ isActive }) =>
              `flex items-center px-6 py-3 text-sm transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white'
                  : 'text-gray-300 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <History className="w-5 h-5 mr-3 flex-shrink-0" />
            <span>{t('sidebar.history')}</span>
          </NavLink>
          <NavLink
            to="/settings"
            className={({ isActive }) =>
              `flex items-center px-6 py-3 text-sm transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white'
                  : 'text-gray-300 hover:bg-gray-800 hover:text-white'
              }`
            }
          >
            <Settings className="w-5 h-5 mr-3 flex-shrink-0" />
            <span>{t('sidebar.settings')}</span>
          </NavLink>
        </div>
      </nav>

      {/* User Info at bottom */}
      <div className="p-6 border-t border-gray-700">
        <div className="text-sm">
          <p className="text-gray-400">{t('sidebar.loggedInAs')}</p>
          <p className="font-medium">{user?.username}</p>
          <p className="text-xs text-gray-500">
            {user?.role === 'admin'
              ? t('common.admin')
              : user?.role === 'auditor'
              ? t('common.auditor')
              : t('common.operator')}
          </p>
        </div>
      </div>
    </aside>
  );
}
