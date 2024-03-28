import React from "react";
import { useNavigate } from "react-router-dom";
import { MenuItem } from "@szhsin/react-menu";

import Dropdown from "./Dropdown";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import useAppsStore from "../store/apps-store";

interface MoreActionsProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
}

export default function MoreActions({ app }: MoreActionsProps) {
  const { uninstallApp, setMirroring, setAutoUpdate } = useAppsStore();
  const navigate = useNavigate();

  const downloaded = Boolean(app.state);

  if (!downloaded) {
    if (!app.metadata) return <div style={{ width: 38 }} />;

    return (
      <Dropdown>
        {app.metadata?.description && (
          <MenuItem
            className="action-entry"
            onClick={() => navigate(`/app-details/${appId(app)}`)}
          >
            View Details
          </MenuItem>
        )}
        {app.metadata?.external_url && (
          <MenuItem>
            <a
              style={{
                color: "inherit",
                whiteSpace: "nowrap",
                cursor: "pointer",
                marginTop: "0.25em",
              }}
              target="_blank"
              href={app.metadata?.external_url}
            >
              View Site
            </a>
          </MenuItem>
        )}
      </Dropdown>
    );
  }

  return (
    <Dropdown>
      <MenuItem
        className="action-entry"
        onClick={() => navigate(`/app-details/${appId(app)}`)}
      >
        View Details
      </MenuItem>
      {app.installed && (
        <>
          <MenuItem className="action-entry" onClick={() => uninstallApp(app)}>
            Uninstall
          </MenuItem>
          <MenuItem
            className="action-entry"
            onClick={() => setMirroring(app, !app.state?.mirroring)}
          >
            {app.state?.mirroring ? "Stop" : "Start"} Mirroring
          </MenuItem>
          <MenuItem
            className="action-entry"
            onClick={() => setAutoUpdate(app, !app.state?.auto_update)}
          >
            {app.state?.auto_update ? "Disable" : "Enable"} Auto Update
          </MenuItem>
        </>
      )}
    </Dropdown>
  );
}
