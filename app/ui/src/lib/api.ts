import { invoke } from "@tauri-apps/api/core";
import type {
  ProjectListResponse,
  StatusResponse,
  PaginatedLogResponse,
  GroupedLogResponse,
  GlobalGroupedLogResponse,
  DiffResponse,
  CatResponse,
  DensityResponse,
  ConfigResponse,
} from "./types";

export async function listProjects(): Promise<ProjectListResponse> {
  return await invoke<ProjectListResponse>("list_projects");
}

export async function selectProject(path: string): Promise<StatusResponse> {
  return await invoke<StatusResponse>("select_project", { path });
}

export async function getProjectStatus(): Promise<StatusResponse> {
  return await invoke<StatusResponse>("get_project_status");
}

export async function removeProject(path: string): Promise<void> {
  await invoke("remove_project", { path });
}

export interface GetLogParams {
  target?: string;
  since?: string;
  limit?: number;
  cursor?: string;
  include?: string[];
  exclude?: string[];
  groupByFile?: boolean;
}

export async function getLog(
  params: GetLogParams & { groupByFile: true }
): Promise<GroupedLogResponse>;
export async function getLog(
  params?: GetLogParams
): Promise<PaginatedLogResponse>;
export async function getLog(
  params: GetLogParams = {}
): Promise<PaginatedLogResponse | GroupedLogResponse> {
  return await invoke("get_log", {
    target: params.target ?? null,
    since: params.since ?? null,
    limit: params.limit ?? null,
    cursor: params.cursor ?? null,
    include: params.include ?? null,
    exclude: params.exclude ?? null,
    groupByFile: params.groupByFile ?? null,
  });
}

export interface GetGlobalLogParams {
  since?: string;
  limit?: number;
  include?: string[];
  exclude?: string[];
  groupByFile?: boolean;
  includeProject?: string[];
  excludeProject?: string[];
}

export async function getGlobalLog(
  params: GetGlobalLogParams & { groupByFile: true }
): Promise<GlobalGroupedLogResponse>;
export async function getGlobalLog(
  params?: GetGlobalLogParams
): Promise<PaginatedLogResponse>;
export async function getGlobalLog(
  params: GetGlobalLogParams = {}
): Promise<PaginatedLogResponse | GlobalGroupedLogResponse> {
  return await invoke("get_global_log", {
    since: params.since ?? null,
    limit: params.limit ?? null,
    include: params.include ?? null,
    exclude: params.exclude ?? null,
    groupByFile: params.groupByFile ?? null,
    includeProject: params.includeProject ?? null,
    excludeProject: params.excludeProject ?? null,
  });
}

export interface GetDiffParams {
  at?: string;
  from?: string;
  to?: string;
  file?: string;
  snapshot?: number;
  project?: string;
}

export async function getDiff(params: GetDiffParams): Promise<DiffResponse> {
  return await invoke<DiffResponse>("get_diff", {
    project: params.project ?? null,
    at: params.at ?? null,
    from: params.from ?? null,
    to: params.to ?? null,
    file: params.file ?? null,
    snapshot: params.snapshot ?? null,
  });
}

export interface GetFileContentParams {
  file: string;
  at?: string;
  snapshot?: number;
  project?: string;
}

export async function getFileContent(
  params: GetFileContentParams
): Promise<CatResponse> {
  return await invoke<CatResponse>("get_file_content", {
    project: params.project ?? null,
    file: params.file,
    at: params.at ?? null,
    snapshot: params.snapshot ?? null,
  });
}

export interface GetDensityParams {
  buckets?: number;
  since?: string;
  include?: string[];
  exclude?: string[];
}

export async function getDensity(
  params: GetDensityParams = {}
): Promise<DensityResponse> {
  return await invoke<DensityResponse>("get_density", {
    buckets: params.buckets ?? null,
    since: params.since ?? null,
    include: params.include ?? null,
    exclude: params.exclude ?? null,
  });
}

export interface GetGlobalDensityParams {
  buckets?: number;
  since?: string;
  include?: string[];
  exclude?: string[];
}

export async function getGlobalDensity(
  params: GetGlobalDensityParams = {}
): Promise<DensityResponse> {
  return await invoke<DensityResponse>("get_global_density", {
    buckets: params.buckets ?? null,
    since: params.since ?? null,
    include: params.include ?? null,
    exclude: params.exclude ?? null,
  });
}

// ---------------------------------------------------------------------------
// Daemon control
// ---------------------------------------------------------------------------

export async function watchProject(path: string): Promise<Record<string, unknown>> {
  return await invoke<Record<string, unknown>>("watch_project", { path });
}

export async function unwatchProject(path: string): Promise<Record<string, unknown>> {
  return await invoke<Record<string, unknown>>("unwatch_project", { path });
}

export async function stopDaemon(): Promise<Record<string, unknown>> {
  return await invoke<Record<string, unknown>>("stop_daemon");
}

export async function restartDaemon(): Promise<Record<string, unknown>> {
  return await invoke<Record<string, unknown>>("restart_daemon");
}

export async function getDaemonStatus(): Promise<Record<string, unknown>> {
  return await invoke<Record<string, unknown>>("get_daemon_status");
}

// ---------------------------------------------------------------------------
// Config and migration
// ---------------------------------------------------------------------------

export async function getConfig(): Promise<ConfigResponse> {
  return await invoke<ConfigResponse>("get_config");
}

export async function moveStorage(path: string): Promise<Record<string, unknown>> {
  return await invoke<Record<string, unknown>>("move_storage", { path });
}
