import React from "react";
import { useNavigate } from "react-router-dom";

import Dropdown from "./Dropdown";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import useAppsStore from "../store/apps-store";

interface MoreActionsProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
  className?: string;
}

export default function MoreActions({ app, className }: MoreActionsProps) {
  const { uninstallApp, setMirroring, setAutoUpdate } = useAppsStore();
  const navigate = useNavigate();

  const downloaded = Boolean(app.state);

  if (!downloaded) {
    if (!app.metadata) return <></>;

    return (
      <Dropdown className={className}>
        <div className="flex flex-col bg-black/50 p-2 rounded-lg">
          {app.metadata?.description && (
            <button
              className="my-1 whitespace-nowrap clear"
              onClick={() => navigate(`/app-details/${appId(app)}`)}
            >
              View Details
            </button>
          )}
          {app.metadata?.external_url && (
            <a
              target="_blank"
              href={app.metadata?.external_url}
              className="mb-1 whitespace-nowrap button clear"
            >
              View Site
            </a>
          )}
        </div>
      </Dropdown>
    );
  }

  return (
    <Dropdown className={className}>
      <div className="flex flex-col bg-black/50 p-2 rounded-lg">
        <button
          className="my-1 whitespace-nowrap clear"
          onClick={() => navigate(`/app-details/${appId(app)}`)}
        >
          View Details
        </button>
        {app.installed && (
          <>
            <button
              className="mb-1 whitespace-nowrap clear"
              onClick={() => uninstallApp(app)}
            >
              Uninstall
            </button>
            <button
              className="mb-1 whitespace-nowrap clear"
              onClick={() => setMirroring(app, !app.state?.mirroring)}
            >
              {app.state?.mirroring ? "Stop" : "Start"} Mirroring
            </button>
            <button
              className="mb-1 whitespace-nowrap clear"
              onClick={() => setAutoUpdate(app, !app.state?.auto_update)}
            >
              {app.state?.auto_update ? "Disable" : "Enable"} Auto Update
            </button>
          </>
        )}
      </div>
    </Dropdown>
  );
}
