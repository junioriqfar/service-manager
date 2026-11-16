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
  webServerService: string;
  phpVersion: string;
  apacheVersion: string;
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
  webServerService: string;
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

export interface RemoteTriggerConfig {
  host: string;
  user: string;
  port: number;
  identity: string;
  authMode: string;
  password: string;
  rememberPassword: boolean;
  passwordProtected: string;
  wrapper: string;
  remoteBin: string;
  remoteConfig: string;
  app: string;
}

export interface RemoteTriggerOutput {
  stream: string;
  line: string;
}

export interface RemoteTriggerResult {
  command: string;
  exitCode: number;
  output: string;
}

export interface ToolPath {
  path: string;
  version: string;
  detectedAt: string;
}

export interface ApacheToolPath extends ToolPath {
  root: string;
  config: string;
}

export interface SettingsTools {
  git: ToolPath;
  php: ToolPath;
  composer: ToolPath;
  node: ToolPath;
  npm: ToolPath;
  pnpm: ToolPath;
  yarn: ToolPath;
  bun: ToolPath;
  go: ToolPath;
  apache: ApacheToolPath;
  nssm: ToolPath;
}

export interface UpdateTimeouts {
  preflightSeconds: number;
  fetchSeconds: number;
  toolSeconds: number;
  cloneSeconds: number;
}

export interface UpdaterDefaults {
  logDir: string;
  maxConcurrentUpdates: number;
  timeouts: UpdateTimeouts;
  logRetentionDays: number;
}

export interface SettingsTrigger {
  allowedClientIPs: string[];
  allowLocal: boolean;
}

export interface PrivilegedExecution {
  mode: string;
  taskName: string;
  userId: string;
  logonType: string;
}

export interface Settings {
  version: number;
  tools: SettingsTools;
  updater: UpdaterDefaults;
  trigger: SettingsTrigger;
  privilegedExecution: PrivilegedExecution;
  variables: Record<string, string>;
}

export interface ToolPathIssue {
  tool: string;
  path: string;
  message: string;
}

export interface SettingsDiagnostics {
  missingRequiredTools: string[];
  invalidPaths: ToolPathIssue[];
  taskInstalled: boolean;
  taskMessage: string;
  taskError: string | null;
  legacyFallback: boolean;
}

export interface SettingsView {
  settings: Settings;
  legacyFallback: boolean;
  legacyTrigger: SettingsTrigger | null;
  currentClientIp: string | null;
  diagnostics: SettingsDiagnostics;
}

export interface ApacheServiceChange {
  name: string;
  oldExecutable: string;
  newExecutable: string;
  oldRoot: string;
  newRoot: string;
}

export interface ApacheAppChange {
  id: string;
  name: string;
}

export interface ApacheChangeView {
  changed: boolean;
  oldPath: string;
  newPath: string;
  oldRoot: string;
  newRoot: string;
  oldConfig: string;
  newConfig: string;
  oldVersion: string;
  newVersion: string;
  services: ApacheServiceChange[];
  apps: ApacheAppChange[];
  createdAt: string;
}

export interface ApacheApplyResult {
  success: boolean;
  services: string[];
  errors: string[];
  auditPath: string;
}

export interface ApacheServiceOption {
  name: string;
  displayName: string;
  executable: string;
  root: string;
  config: string;
}

export interface ApacheMappingSuggestion {
  appId: string;
  appName: string;
  localPath: string;
  documentRoot: string;
  configFile: string;
  matchKind: "exact" | "descendant" | "ancestor" | "";
  confidence: "high" | "medium" | "";
  existingService: string;
}

export interface ApacheMappingScanView {
  serviceName: string;
  services: ApacheServiceOption[];
  suggestions: ApacheMappingSuggestion[];
  alreadyMapped: ApacheMappingSuggestion[];
  warnings: string[];
  error: string | null;
}

export interface PhpReferenceView {
  appId: string;
  appName: string;
  field: string;
  value: string;
}

export interface PhpPathChangeView {
  oldPath: string;
  oldRoot: string;
  newPath: string;
  newRoot: string;
  references: PhpReferenceView[];
}

export interface SettingsSaveResult {
  settings: Settings;
  apachePending: ApacheChangeView | null;
  phpPathChange: PhpPathChangeView | null;
}

export interface ToolDetection {
  tool: string;
  found: boolean;
  success: boolean;
  path: string;
  version: string;
  root: string;
  config: string;
  message: string;
}

export interface ToolTest {
  tool: string;
  path: string;
  version: string;
  success: boolean;
  output: string;
  error: string | null;
}

export interface TaskStatus {
  taskName: string;
  installed: boolean;
  state: string;
  runLevel: string;
  user: string;
  logonType: string;
  lastRun: string;
  lastResult: number;
  error: string | null;
}

export interface TaskOperation {
  success: boolean;
  exitCode: number;
  output: string;
}

