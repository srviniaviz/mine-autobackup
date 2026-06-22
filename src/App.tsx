import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  ArrowLeft,
  CheckCircle2,
  CheckSquare,
  Clock3,
  Cloud,
  FolderOpen,
  LogOut,
  Loader2,
  Pause,
  Pickaxe,
  Play,
  Settings,
  ShieldCheck,
  Square,
  X
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

type BackupStatus = {
  minecraft_dir: string | null;
  backup_dir: string | null;
  worlds: MinecraftWorld[];
  selected_worlds: string[];
  google_connected: boolean;
  google_email: string | null;
  interval_minutes: number;
  auto_enabled: boolean;
  is_running: boolean;
  progress: BackupProgress;
  running_since: string | null;
  last_backup_at: string | null;
  next_backup_at: string | null;
  last_result: string | null;
};

type BackupProgress = {
  current: number;
  total: number;
  label: string;
  updated_at: string | null;
};

type MinecraftWorld = {
  id: string;
  name: string;
  size_bytes: number;
  modified_at: string | null;
};

const defaultStatus: BackupStatus = {
  minecraft_dir: null,
  backup_dir: null,
  worlds: [],
  selected_worlds: [],
  google_connected: false,
  google_email: null,
  interval_minutes: 60,
  auto_enabled: false,
  is_running: false,
  progress: {
    current: 0,
    total: 0,
    label: "",
    updated_at: null
  },
  running_since: null,
  last_backup_at: null,
  next_backup_at: null,
  last_result: null
};

function shortPath(path?: string | null) {
  if (!path) return "Nao configurado";
  const parts = path.replace(/\\/g, "/").split("/");
  if (parts.length <= 3) return path;
  return `${parts[0]}/.../${parts.slice(-2).join("/")}`;
}

function formatDate(value?: string | null) {
  if (!value) return "Ainda nao";
  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  }).format(new Date(value));
}

function formatWorldDate(value?: string | null) {
  if (!value) return "Sem data";
  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    year: "2-digit"
  }).format(new Date(value));
}

function formatBytes(bytes: number) {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

function formatInterval(minutes: number) {
  if (minutes >= 1440 && minutes % 1440 === 0) return `${minutes / 1440}d`;
  if (minutes >= 60 && minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

function formatElapsed(value?: string | null) {
  if (!value) return "";
  const seconds = Math.max(0, Math.floor((Date.now() - new Date(value).getTime()) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${minutes}m ${rest}s`;
}

function App() {
  const [status, setStatus] = useState<BackupStatus>(defaultStatus);
  const [busy, setBusy] = useState(false);
  const [screen, setScreen] = useState<"home" | "settings">("home");
  const [message, setMessage] = useState("Pronto para proteger seus mundos");
  const [, setClockTick] = useState(0);
  const wasConnected = useRef(false);
  const needsLogin = !status.google_connected;
  const visibleScreen = needsLogin ? "login" : screen;

  async function refresh() {
    const next = await invoke<BackupStatus>("get_status");
    setStatus(next);
  }

  useEffect(() => {
    refresh().catch(() => setMessage("Nao consegui carregar as configuracoes"));
    const timer = window.setInterval(() => {
      refresh().catch(() => undefined);
    }, 2500);
    return () => window.clearInterval(timer);
  }, []);

  async function pickMinecraftDir() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Escolha a pasta .minecraft"
    });
    if (typeof selected !== "string") return;
    await invoke("set_minecraft_dir", { path: selected });
    setMessage("Pasta .minecraft configurada");
    await refresh();
  }

  async function setIntervalMinutes(minutes: number) {
    await invoke("set_interval_minutes", { minutes });
    setMessage(`Periodicidade ajustada para ${minutes} min`);
    await refresh();
  }

  async function setSelectedWorlds(worlds: string[]) {
    await invoke("set_selected_worlds", { worlds });
    await refresh();
  }

  async function toggleWorld(worldId: string) {
    const selected = new Set(status.selected_worlds);
    if (selected.has(worldId)) {
      selected.delete(worldId);
    } else {
      selected.add(worldId);
    }
    setMessage(`${selected.size} mundo(s) selecionado(s)`);
    await setSelectedWorlds([...selected]);
  }

  async function selectAllWorlds() {
    const all = status.worlds.map((world) => world.id);
    setMessage(`${all.length} mundo(s) selecionado(s)`);
    await setSelectedWorlds(all);
  }

  async function clearWorlds() {
    setMessage("Nenhum mundo selecionado");
    await setSelectedWorlds([]);
  }

  async function connectGoogle() {
    try {
      setBusy(true);
      setMessage("Abrindo login do Google...");
      await invoke("google_login");
      setMessage("Finalize o login no navegador...");
      await refresh();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function disconnectGoogle() {
    await invoke("google_logout");
    setMessage("Google Drive desconectado");
    await refresh();
  }

  async function toggleAuto() {
    await invoke("set_auto_enabled", { enabled: !status.auto_enabled });
    setMessage(!status.auto_enabled ? "Backup automatico ligado" : "Backup automatico pausado");
    await refresh();
  }

  async function runBackup() {
    try {
      setBusy(true);
      setMessage("Compactando mundos...");
      const result = await invoke<string>("run_backup_now");
      setMessage(result);
      await refresh();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function hideWindow() {
    await invoke("hide_window");
  }

  const selectedCount = status.selected_worlds.length;
  const ready = Boolean(status.minecraft_dir && status.google_connected && selectedCount > 0);
  const intervalOptions = useMemo(() => [15, 30, 60, 180, 360, 720, 1440], []);
  const progressPercent =
    status.progress.total > 0
      ? Math.min(100, Math.round((status.progress.current / status.progress.total) * 100))
      : 0;
  const totalElapsed = formatElapsed(status.running_since);
  const phaseElapsed = formatElapsed(status.progress.updated_at);
  const progressLabel = status.progress.label.toLowerCase();
  const isUploadStep = progressLabel.startsWith("enviando");
  const isDrivePrepStep =
    progressLabel.includes("autenticando") ||
    progressLabel.includes("localizando") ||
    progressLabel.includes("iniciando") ||
    progressLabel.includes("preparando google");

  useEffect(() => {
    if (!wasConnected.current && status.google_connected) {
      setScreen("home");
      setMessage("Google Drive conectado");
    }
    wasConnected.current = status.google_connected;
  }, [status.google_connected]);

  useEffect(() => {
    if (!status.is_running) return;
    const timer = window.setInterval(() => setClockTick((tick) => tick + 1), 1000);
    return () => window.clearInterval(timer);
  }, [status.is_running]);

  return (
    <main className="min-h-screen overflow-hidden bg-[#111111] text-white">
      <section className="minecraft-shell relative flex min-h-screen flex-col overflow-hidden border-2 border-black px-4 py-3 shadow-2xl">
        <div className="sky-band" />
        <div className="terrain-strip" />
        <div className="ore-glow" />

        <header className="relative z-10 flex items-start justify-between">
          <div>
            <div className="flex items-center gap-2">
              <span className="minecraft-badge">
                <Pickaxe size={21} />
              </span>
              <div>
                <h1 className="minecraft-title">
                  {visibleScreen === "login"
                    ? "Conectar Drive"
                    : visibleScreen === "settings"
                      ? "Configuracoes"
                      : "Mine AutoBackup"}
                </h1>
                <p className="minecraft-kicker">
                  {visibleScreen === "login"
                    ? "Login obrigatorio"
                    : visibleScreen === "settings"
                      ? "Ajustes do backup"
                      : "Protecao dos mundos"}
                </p>
              </div>
            </div>
          </div>

          <div className="flex gap-2">
            {visibleScreen === "login" ? null : visibleScreen === "settings" ? (
              <button className="icon-button" title="Voltar" onClick={() => setScreen("home")}>
                <ArrowLeft size={16} />
              </button>
            ) : (
              <button className="icon-button" title="Configuracoes" onClick={() => setScreen("settings")}>
                <Settings size={16} />
              </button>
            )}
            <button className="icon-button" title="Minimizar para bandeja" onClick={hideWindow}>
              <X size={16} />
            </button>
          </div>
        </header>

        {visibleScreen === "login" ? (
          <>
            <div className="relative z-10 mt-4 border-2 border-black bg-[#1f1f1f] p-3 shadow-pixel">
              <p className="text-[11px] font-black uppercase tracking-[0.16em] text-[#d7d7d7]">
                Google Drive necessario
              </p>
              <p className="mt-2 text-xs font-semibold leading-5 text-[#b8b8b8]">
                Entre com sua conta Google para salvar os backups direto no seu Drive. Depois do login,
                voce libera a tela de mundos e configuracoes.
              </p>
            </div>

            <div className="world-panel relative z-10 mt-4">
              <div className="mb-3 flex items-center gap-2 text-[#86d562]">
                <Cloud size={20} />
                <div>
                  <p className="text-xs font-black uppercase tracking-[0.16em] text-white">
                    Conta Google Drive
                  </p>
                  <p className="text-[11px] font-bold text-[#f2b84b]">Pendente</p>
                </div>
              </div>

              <button className="grass-button w-full" disabled={busy} onClick={connectGoogle}>
                {busy ? <Loader2 className="animate-spin" size={18} /> : <Cloud size={18} />}
                Conectar Drive
              </button>

              <p className="mt-3 text-[11px] font-bold leading-4 text-[#9f9f9f]">
                O app cria uma pasta Mine AutoBackup no Drive e envia apenas os arquivos de backup
                gerados por ele.
              </p>
            </div>
          </>
        ) : visibleScreen === "home" ? (
          <>
            <div className="relative z-10 mt-4 border-2 border-black bg-[#1f1f1f] p-2 shadow-pixel">
              <p className="text-[11px] font-black uppercase tracking-[0.16em] text-[#d7d7d7]">
                Mundos do Minecraft
              </p>
              <p className="mt-1 text-xs font-semibold leading-4 text-[#b8b8b8]">
                Marque os saves que entram no proximo backup.
              </p>
            </div>

            <div className="world-panel relative z-10 mt-4">
              <div className="mb-2 flex items-center justify-between">
                <div>
                  <p className="text-xs font-black uppercase tracking-[0.16em] text-white">
                    Saves encontrados
                  </p>
                  <p className="text-[11px] font-bold text-[#9f9f9f]">
                    {selectedCount} de {status.worlds.length} selecionado(s)
                  </p>
                </div>
                <div className="flex gap-2">
                  <button className="tiny-button" disabled={status.worlds.length === 0} onClick={selectAllWorlds}>
                    Todos
                  </button>
                  <button className="tiny-button" disabled={selectedCount === 0} onClick={clearWorlds}>
                    Nenhum
                  </button>
                </div>
              </div>

              <div className="world-list">
                {status.worlds.length === 0 ? (
                  <div className="empty-worlds">
                    {status.minecraft_dir ? "Nenhum mundo com level.dat encontrado." : "Abra as configuracoes e escolha a pasta .minecraft."}
                  </div>
                ) : (
                  status.worlds.map((world) => {
                    const selected = status.selected_worlds.includes(world.id);
                    return (
                      <button
                        key={world.id}
                        className={`world-row ${selected ? "world-row-selected" : ""}`}
                        onClick={() => toggleWorld(world.id)}
                        title={world.id}
                      >
                        {selected ? <CheckSquare size={17} /> : <Square size={17} />}
                        <span className="min-w-0 flex-1 truncate text-left">{world.name}</span>
                        <small className="world-meta">
                          <span>{formatBytes(world.size_bytes)}</span>
                          <span>{formatWorldDate(world.modified_at)}</span>
                        </small>
                      </button>
                    );
                  })
                )}
              </div>
            </div>

            <div className="relative z-10 mt-4 grid grid-cols-[1fr_auto] gap-2">
              <button
                className="grass-button"
                disabled={!ready || busy || status.is_running}
                onClick={runBackup}
              >
                {busy || status.is_running ? <Loader2 className="animate-spin" size={18} /> : <Archive size={18} />}
                Backup agora
              </button>
              <button className="toggle-button" disabled={!ready} onClick={toggleAuto} title="Ligar backup automatico">
                {status.auto_enabled ? <Pause size={18} /> : <Play size={18} />}
              </button>
            </div>

            {status.is_running && (
              <div className="progress-panel relative z-10 mt-4">
                <div className="mb-2 flex min-w-0 items-center justify-between gap-3 text-[11px] font-black uppercase tracking-[0.16em] text-white">
                  <span className="min-w-0 flex-1 truncate">{status.progress.label || "Fazendo backup"}</span>
                  <span className="text-[#86d562]">{progressPercent}%</span>
                </div>
                <div className="progress-track">
                  <div className="progress-fill" style={{ width: `${progressPercent}%` }} />
                </div>
                <p className="mt-2 text-[11px] font-bold leading-4 text-[#b8b8b8]">
                  {isUploadStep
                    ? `Upload em blocos${phaseElapsed ? ` ha ${phaseElapsed}` : ""}. Total ${totalElapsed}.`
                    : isDrivePrepStep
                      ? `Preparando conexao com o Drive${phaseElapsed ? ` ha ${phaseElapsed}` : ""}. Limite: 30s.`
                      : `Em andamento${totalElapsed ? ` ha ${totalElapsed}` : ""}.`}
                </p>
              </div>
            )}

            <div className="relative z-10 mt-4 grid grid-cols-2 gap-2">
              <div className="stat-block">
                <CheckCircle2 size={16} />
                <span>Ultimo</span>
                <strong>{formatDate(status.last_backup_at)}</strong>
              </div>
              <div className="stat-block">
                <Clock3 size={16} />
                <span>Proximo</span>
                <strong>{formatDate(status.next_backup_at)}</strong>
              </div>
            </div>
          </>
        ) : (
          <>
            <div className="relative z-10 mt-4 border-2 border-black bg-[#1f1f1f] p-2 shadow-pixel">
              <p className="text-[11px] font-black uppercase tracking-[0.16em] text-[#d7d7d7]">
                Pastas e agenda
              </p>
              <p className="mt-1 text-xs font-semibold leading-4 text-[#b8b8b8]">
                Configure a origem dos saves, o destino e quando o backup automatico roda.
              </p>
            </div>

            <div className="relative z-10 mt-3">
              <button className="stone-button" onClick={pickMinecraftDir}>
                <FolderOpen size={16} />
                Escolher pasta .minecraft
              </button>
            </div>

            <div className="relative z-10 mt-4 space-y-3">
              <div className="info-block">
                <span>Google Drive</span>
                <strong title={status.google_email ?? ""}>
                  {status.google_connected ? status.google_email ?? "Conta conectada" : "Nao conectado"}
                </strong>
              </div>
              <div className="info-block">
                <span>Pasta do jogo</span>
                <strong title={status.minecraft_dir ?? ""}>{shortPath(status.minecraft_dir)}</strong>
              </div>
            </div>

            <div className="relative z-10 mt-4 border-2 border-black bg-[#2b2b2b] p-3 shadow-pixel">
              <div className="mb-2 flex items-center justify-between text-xs font-black uppercase tracking-[0.16em] text-[#f2f2f2]">
                <span>Conta Google Drive</span>
                <span className={status.google_connected ? "text-[#86d562]" : "text-[#f2b84b]"}>
                  {status.google_connected ? "Conectado" : "Pendente"}
                </span>
              </div>
              <p className="mb-3 text-xs font-bold leading-4 text-[#b8b8b8]">
                {status.google_connected
                  ? `Backups serao enviados para ${status.google_email ?? "sua conta Google"}.`
                  : "Entre com sua conta para enviar os backups direto para o Drive."}
              </p>
              <div className="mt-3 grid grid-cols-[1fr_auto] gap-2">
                <button className="grass-button h-10" disabled={busy} onClick={connectGoogle}>
                  <Cloud size={17} />
                  {status.google_connected ? "Reconectar Drive" : "Conectar Drive"}
                </button>
                <button className="toggle-button h-10 w-10" disabled={!status.google_connected} onClick={disconnectGoogle} title="Desconectar">
                  <LogOut size={17} />
                </button>
              </div>
            </div>

            <div className="relative z-10 mt-4 border-2 border-black bg-[#2b2b2b] p-3 shadow-pixel">
              <div className="mb-2 flex items-center justify-between text-xs font-black uppercase tracking-[0.16em] text-[#f2f2f2]">
                <span>Periodicidade</span>
                <span className="text-[#86d562]">{formatInterval(status.interval_minutes)}</span>
              </div>
              <div className="grid grid-cols-4 gap-2">
                {intervalOptions.map((minutes) => (
                  <button
                    key={minutes}
                    className={`mini-button ${status.interval_minutes === minutes ? "mini-button-active" : ""}`}
                    onClick={() => setIntervalMinutes(minutes)}
                  >
                    {formatInterval(minutes)}
                  </button>
                ))}
              </div>
            </div>
          </>
        )}

        <div className="status-board relative z-10 mt-auto p-3">
          <div className="flex items-center gap-2 text-sm">
            <span className={`h-3 w-3 border border-black ${ready ? "bg-[#86d562]" : "bg-[#f2b84b]"}`} />
            <span className="font-black">{message}</span>
          </div>
          {!status.is_running && status.last_result && (
            <p className="mt-2 line-clamp-2 text-xs text-[#9f9f9f]">{status.last_result}</p>
          )}
        </div>

        <footer className="relative z-10 mt-3 flex items-center justify-between text-[11px] font-black uppercase tracking-[0.16em] text-[#8f8f8f]">
          <span className="flex items-center gap-1">
            <ShieldCheck size={13} />
            {status.auto_enabled ? `Auto ${formatInterval(status.interval_minutes)}` : "Auto pausado"}
          </span>
          <span>v0.1.0</span>
        </footer>
      </section>
    </main>
  );
}

export default App;
