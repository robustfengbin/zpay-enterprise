import React, { ReactNode } from 'react';
import { Sidebar } from './Sidebar';
import { Header } from './Header';

interface LayoutProps {
  children: ReactNode;
}

export function Layout({ children }: LayoutProps) {
  return (
    <div className="flex h-screen bg-canvas">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <Header />
        <main className="flex-1 overflow-y-auto">
          {/* One measure for the whole product — content never stretches the
              full width of an ultrawide display. */}
          <div className="mx-auto w-full max-w-[1240px] px-7 py-7">{children}</div>
        </main>
      </div>
    </div>
  );
}
