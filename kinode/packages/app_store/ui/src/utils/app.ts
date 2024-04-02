import { AppInfo } from "../types/Apps";

export const appId = (app: AppInfo) => `${app.package}:${app.publisher}`

export const getAppName = (app: AppInfo) => app.metadata?.name || appId(app)

export enum AppType {
    Downloaded = 'downloaded',
    Installed = 'installed',
    Local = 'local',
    System = 'system',
}

export const getAppType = (app: AppInfo) => {
  if (app.publisher === 'sys') {
    return AppType.System
  } else if (app.state?.our_version && !app.state?.capsApproved) {
    return AppType.Downloaded
  } else if (!app.metadata) {
    return AppType.Local
  } else {
    return AppType.Installed
  }
}
