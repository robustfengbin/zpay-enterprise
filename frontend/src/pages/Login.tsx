import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { AlertCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '../hooks/useAuth';
import { LoadingSpinner } from '../components/Common';

export function Login() {
  const { t, i18n } = useTranslation();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const { login } = useAuth();
  const navigate = useNavigate();
  const isZh = i18n.language.startsWith('zh');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setIsLoading(true);

    try {
      await login(username, password);
      navigate('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : t('login.failed'));
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-rail-900 px-4">
      {/* A quiet radial wash keeps the sign-in page from reading as a blank slab. */}
      <div
        aria-hidden="true"
        className="pointer-events-none fixed inset-0"
        style={{
          background:
            'radial-gradient(60rem 40rem at 50% -10%, rgba(95,109,242,0.22), transparent 65%)',
        }}
      />

      <div className="relative w-full max-w-[380px]">
        <div className="mb-6 flex items-center justify-center gap-2.5">
          <span className="flex h-9 w-9 items-center justify-center rounded-[10px] bg-brand-600 text-base font-bold text-white shadow-[0_4px_14px_rgba(75,86,221,0.5)]">
            z
          </span>
          <div className="leading-tight">
            <p className="text-[0.9375rem] font-semibold tracking-[-0.01em] text-white">
              {t('sidebar.title')}
            </p>
            <p className="text-[0.6875rem] text-[#7b8699]">{t('sidebar.subtitle')}</p>
          </div>
        </div>

        <div className="rounded-[14px] border border-white/10 bg-surface p-7 shadow-[0_24px_60px_-20px_rgba(0,0,0,0.55)]">
          <h1 className="text-lg font-semibold tracking-[-0.01em] text-ink-900">
            {t('login.title')}
          </h1>
          <p className="mt-1 text-[0.8125rem] text-ink-400">{t('login.subtitle')}</p>

          {error && (
            <div className="alert alert-bad mt-5">
              <AlertCircle className="mt-px h-4 w-4 shrink-0" />
              <span>{error}</span>
            </div>
          )}

          <form onSubmit={handleSubmit} className="mt-5 space-y-4">
            <div>
              <label className="label" htmlFor="username">
                {t('login.username')}
              </label>
              <input
                id="username"
                type="text"
                autoComplete="username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                className="field"
                placeholder={t('login.usernamePlaceholder')}
                required
              />
            </div>

            <div>
              <label className="label" htmlFor="password">
                {t('login.password')}
              </label>
              <input
                id="password"
                type="password"
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="field"
                placeholder={t('login.passwordPlaceholder')}
                required
              />
            </div>

            <button type="submit" disabled={isLoading} className="btn-primary w-full">
              {isLoading ? <LoadingSpinner size="sm" className="py-0" /> : t('login.signIn')}
            </button>
          </form>

          <div className="mt-6 flex items-center justify-between border-t border-line-100 pt-4">
            <p className="text-[0.6875rem] text-ink-400">
              {t('login.auditor_hint')}{' '}
              <a href="/auditor/login" className="font-medium text-brand-600 hover:underline">
                {t('login.auditor_link')}
              </a>
            </p>
            <button
              type="button"
              onClick={() => void i18n.changeLanguage(isZh ? 'en' : 'zh')}
              className="btn-ghost btn-sm"
            >
              {isZh ? 'EN' : '中文'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
