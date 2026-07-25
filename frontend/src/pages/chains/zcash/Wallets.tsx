import React, { useEffect, useState } from 'react';
import { ChainWallets } from '../components/ChainWallets';
import { MigrationBanner } from '../../Migration';
import { walletService } from '../../../services/api';
import { useAuth } from '../../../hooks/useAuth';

export function ZcashWallets() {
  const { user } = useAuth();
  const [wallets, setWallets] = useState<Array<{ id: number; name: string }>>([]);

  // F4.1 — surface the Ironwood migration banner for every Zcash wallet
  // that still holds legacy-pool shielded funds. Each banner fetches its
  // own status and renders nothing when there is nothing to migrate, so
  // this stays purely advisory on top of the shared wallet list.
  useEffect(() => {
    if (user?.role !== 'admin') return;
    let alive = true;
    walletService
      .listWallets('zcash')
      .then((ws) => {
        if (alive) {
          setWallets(ws.filter((w) => w.chain === 'zcash').map((w) => ({ id: w.id, name: w.name })));
        }
      })
      .catch(() => {
        /* banner is advisory — the wallet list below still loads */
      });
    return () => {
      alive = false;
    };
  }, [user?.role]);

  return (
    <ChainWallets
      chainId="zcash"
      banner={wallets.map((w) => (
        <MigrationBanner key={w.id} walletId={w.id} walletName={w.name} />
      ))}
    />
  );
}
