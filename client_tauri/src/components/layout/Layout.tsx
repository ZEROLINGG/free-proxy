import { useState, type ReactNode } from "react";
import { useUi } from "../../store/ui";
import { About } from "../../pages/About";
import { CaCert } from "../../pages/CaCert";
import { Dashboard } from "../../pages/Dashboard";
import { ProxySettingsPage } from "../../pages/ProxySettings";
import { SpeedTest } from "../../pages/SpeedTest";
import { BottomTabs } from "./BottomTabs";
import { GlassTopbar } from "./GlassTopbar";
import { Sidebar } from "./Sidebar";

const pages: Record<string, () => ReactNode> = {
  dashboard: () => <Dashboard />,
  proxy: () => <ProxySettingsPage />,
  speed: () => <SpeedTest />,
  ca: () => <CaCert />,
  about: () => <About />,
};

export function Layout() {
  const view = useUi((s) => s.view);
  const [scrolled, setScrolled] = useState(false);

  return (
    <div className="flex h-screen overflow-hidden bg-ground text-ink">
      <div className="hidden md:block">
        <Sidebar />
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <GlassTopbar scrolled={scrolled} />
        <main
          onScroll={(e) => {
            const t = e.currentTarget;
            setScrolled(t.scrollTop > 8);
          }}
          className="flex-1 overflow-y-auto overscroll-contain"
        >
          <div className="mx-auto max-w-[720px] px-[22px] pb-16 pt-6">
            {pages[view]()}
          </div>
        </main>
        <div className="md:hidden">
          <BottomTabs />
        </div>
      </div>
    </div>
  );
}
