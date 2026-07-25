import React from 'react';
import { LogOut } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { useAuth } from '../../hooks/useAuth';

/** Path prefix → sidebar label key; the longest match wins. */
const SECTION_KEYS: Array<[string, string]> = [
  ['/ethereum/transfer', 'sidebar.transfer_eth'],
  ['/ethereum/wallets', 'sidebar.wallets'],
  ['/ethereum/rpc', 'sidebar.rpcSettings'],
  ['/zcash/transfer', 'sidebar.transfer_zec'],
  ['/zcash/wallets', 'sidebar.wallets'],
  ['/zcash/rpc', 'sidebar.rpcSettings'],
  ['/batch-transfers', 'sidebar.batch_transfers'],
  ['/migrations', 'sidebar.migrations'],
  ['/approval/queue', 'sidebar.approval_queue'],
  ['/approval/pending', 'sidebar.my_approvals'],
  ['/approval/policies', 'sidebar.approval_policies'],
  ['/auditor/manage', 'sidebar.auditor_manage'],
  ['/payroll/runs', 'sidebar.payroll_runs'],
  ['/payroll/employees', 'sidebar.employees'],
  ['/history', 'sidebar.history'],
  ['/settings', 'sidebar.settings'],
];

/**
 * Utility bar: where you are on the left, who you are on the right. The
 * language toggle lives here because the product ships bilingual and the
 * browser's guess is often wrong for a finance team.
 */
export function Header() {
  const { t, i18n } = useTranslation();
  const { user, logout } = useAuth();
  const navigate = useNavigate();
  const { pathname } = useLocation();

  const match = SECTION_KEYS.filter(([p]) => pathname.startsWith(p)).sort(
    (a, b) => b[0].length - a[0].length,
  )[0];
  const section = match ? t(match[1]) : t('sidebar.dashboard');

  const handleLogout = async () => {
    await logout();
    navigate('/login');
  };

  const isZh = i18n.language.startsWith('zh');

  const roleLabel =
    user?.role === 'admin'
      ? t('common.admin')
      : user?.role === 'auditor'
        ? t('common.auditor')
        : t('common.operator');

  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-line-200 bg-surface px-7">
      <nav className="flex items-center gap-2 text-[0.8125rem]" aria-label="breadcrumb">
        <span className="text-ink-300">zPay</span>
        <span className="text-line-300">/</span>
        <span className="font-medium text-ink-700">{section}</span>
      </nav>

      <div className="flex items-center gap-2">
        <button
          onClick={() => void i18n.changeLanguage(isZh ? 'en' : 'zh')}
          className="btn-ghost btn-sm"
          title={isZh ? 'Switch to English' : '切换到中文'}
        >
          {isZh ? 'EN' : '中文'}
        </button>

        <div className="mx-1 h-5 w-px bg-line-200" />

        <div className="flex items-center gap-2.5">
          <span className="flex h-7 w-7 items-center justify-center rounded-full bg-brand-50 text-[0.6875rem] font-semibold uppercase text-brand-700">
            {user?.username?.slice(0, 2) ?? '?'}
          </span>
          <div className="leading-tight">
            <p className="text-[0.8125rem] font-medium text-ink-900">{user?.username}</p>
            <p className="text-[0.6875rem] text-ink-400">{roleLabel}</p>
          </div>
        </div>

        <button
          onClick={handleLogout}
          className="btn-ghost btn-icon btn-sm"
          title={t('header.logout')}
          aria-label={t('header.logout')}
        >
          <LogOut className="h-4 w-4" />
        </button>
      </div>
    </header>
  );
}
