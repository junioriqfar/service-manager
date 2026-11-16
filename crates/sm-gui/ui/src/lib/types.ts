export type ServiceType = "nssm" | "apache" | "postgresql";

export type StartupMode = "auto" | "delayed" | "manual";

export interface ServiceConfig {
  name: string;
  displayName: string;
  serviceType: ServiceType;
  executable: string;
  arguments: string;
  workingDirectory: string;
  apacheRoot: string;
  apacheConfig: string;
  pgData: string;
  pgAccount: string;
  stdoutLog: string;
  stderrLog: string;
  startup: StartupMode;
  restartOnCrash: boolean;
  restartDelaySec: number;
  dependencies: string[];
  addedAt: string;
  updatedAt: string;
}

export interface ServiceRow extends ServiceConfig {
  installed: boolean;
  state: string;
  stateLabel: string;
  pid: number;
  error: string | null;
}

export interface LogView {
  path: string;
  text: string;
}

export interface PathsView {
  home: string;
  servicesPath: string;
  projectsPath: string;
  nssmPath: string;
  appLog: string;
}

export interface BatchStartedEvent {
  action: string;
  total: number;
}

export interface BatchProgressEvent {
  action: string;
  name: string;
  state: string;
  message: string | null;
  done: number;
  total: number;
}

export interface BatchOutcome {
  name: string;
  state: string;
  message: string | null;
}

export interface BatchSummary {
  action: string;
  succeeded: number;
  failed: number;
  cancelled: number;
  outcomes: BatchOutcome[];
}

export interface BatchLogLine {
  state: string;
  name: string;
  message: string | null;
}

export interface BatchView {
  open: boolean;
  action: string;
  total: number;
  done: number;
  text: string;
  lines: BatchLogLine[];
  cancelling: boolean;
  finished: boolean;
  summary: BatchSummary | null;
}

export type ServiceAction = "install" | "start" | "stop" | "restart" | "log" | "delete";

export type BatchActionName = "start" | "stop" | "restart";

export interface UpdaterServicePlan {
  stopBeforePull: string[];
  stopBeforeBuild: string[];
  restartAfter: string[];
  startAfter: string[];
  waitTimeoutSeconds: number;
}

export interface UpdaterHealthCheck {
  enabled: boolean;
  url: string;
  timeoutSeconds: number;
}

export interface UpdaterNotify {
  webhookUrl: string;
  on: string[];
}

export interface UpdaterApp {
  id: string;
  name: string;
  enabled: boolean;
  repoUrl: string;
  localPath: string;
  branch: string;
  preset: string;
  packageManager: string;
  composer: string;
  artisan: string;
  packageInstall: string;
  build: string;
  installCommand: string;
  buildCommand: string;
  queueRestart: boolean;
  cloneOnFirstRun: boolean;
  allowedClientIPs: string[];
  services: UpdaterServicePlan;
  healthCheck: UpdaterHealthCheck;
  notify: UpdaterNotify;
  status: string;
  lastCheckedAt: string;
  lastUpdatedAt: string;
  lastCommit: string;
  lastError: string;
}

export interface UpdaterAppInput {
  id: string;
  name: string;
  enabled: boolean;
  repoUrl: string;
  localPath: string;
  branch: string;
  preset: string;
  packageManager: string;
  composer: string;
  artisan: string;
  packageInstall: string;
  build: string;
  installCommand: string;
  buildCommand: string;
  queueRestart: boolean;
  cloneOnFirstRun: boolean;
  allowedClientIPs: string[];
  services: UpdaterServicePlan;
  healthCheck: UpdaterHealthCheck;
  notify: UpdaterNotify;
}

export interface UpdaterRunOutcome {
  appId: string;
  status: string;
  message: string;
  forbidden: boolean;
}

export interface UpdaterLogView {
  path: string;
  text: string;
}

