import React, { useEffect, useState } from 'react';
import { ChainWallets } from '../components/ChainWallets';
import { MigrationBanner } from '../../Migration';
import { walletService } from '../../../services/api';
import { useAuth } from '../../../hooks/useAuth';

export function ZcashWallets() {
  const { user } = useAuth();
  const [walletIds, setWalletIds] = useState<number[]>([]);

  // F4.1 — surface the Ironwood migration banner for every Zcash wallet
  // that still holds legacy-pool shielded funds. Each banner fetches its
  // own status and renders nothing when there is nothing to migrate, so
  // this stays purely advisory on top of the shared wallet list.
  useEffect(() => {
    if (user?.role !== 'admin') return;
    let alive = true;
    walletService
      .listWallets('zcash')
      .then(ws => { if (alive) setWalletIds(ws.filter(w => w.chain === 'zcash').map(w => w.id)); })
      .catch(() => { /* banner is advisory — the wallet list below still loads */ });
    return () => { alive = false; };
  }, [user?.role]);

  return (
    <div>
      {walletIds.length > 0 && (
        <div className="px-6 pt-4 space-y-2">
          {walletIds.map(id => <MigrationBanner key={id} walletId={id} />)}
        </div>
      )}
      <ChainWallets chainId="zcash" />
    </div>
  );
}
