import { useEffect } from "react";
import { Layout } from "./components/layout/Layout";
import { ToastViewport } from "./components/ui/Toast";
import { useProxy } from "./store/proxy";
import { useSettings } from "./store/settings";
import { applyTheme, useUi } from "./store/ui";

function App() {
  const load = useSettings((s) => s.load);
  const refresh = useProxy((s) => s.refresh);
  const theme = useUi((s) => s.theme);

  useEffect(() => {
    applyTheme(theme);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (useUi.getState().theme === "auto") applyTheme("auto");
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);

  useEffect(() => {
    void load();
    void refresh();
  }, [load, refresh]);

  return (
    <>
      <Layout />
      <ToastViewport />
    </>
  );
}

export default App;
