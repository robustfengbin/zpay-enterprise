import React from 'react';
import { useTranslation } from 'react-i18next';
import type { TransferStatusExt } from '../../types/approval';
import { TRANSFER_STATUS_PALETTE } from '../../types/approval';

interface ExtendedStatusBadgeProps {
  status: TransferStatusExt;
}

/**
 * Badge that handles the F2.1-extended 8-state transfer status.
 * The original 4 states (pending/submitted/confirmed/failed) are preserved
 * for backward-compatibility via TRANSFER_STATUS_PALETTE mapping.
 * The existing StatusBadge.tsx is left untouched per F2.1 NFR-2.
 */
export function ExtendedStatusBadge({ status }: ExtendedStatusBadgeProps) {
  const { t } = useTranslation();
  const meta = TRANSFER_STATUS_PALETTE[status];

  const bgByColor: Record<string, string> = {
    blue:   'bg-blue-100',
    green:  'bg-green-100',
    yellow: 'bg-yellow-100',
    red:    'bg-red-100',
  };
  const textByColor: Record<string, string> = {
    blue:   'text-blue-800',
    green:  'text-green-800',
    yellow: 'text-yellow-800',
    red:    'text-red-800',
  };

  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${bgByColor[meta.color]} ${textByColor[meta.color]}`}
    >
      {t(meta.label_key)}
    </span>
  );
}
