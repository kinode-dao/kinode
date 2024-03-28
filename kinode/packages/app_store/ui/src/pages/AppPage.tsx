import React, { useState, useEffect, useMemo, useCallback } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import ActionButton from "../components/ActionButton";
import AppHeader from "../components/AppHeader";
import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { appId } from "../utils/app";

interface AppPageProps extends PageProps {}

export default function AppPage(props: AppPageProps) {
  // eslint-disable-line
  const { myApps, listedApps, getListedApp } = useAppsStore();
  const navigate = useNavigate();
  const params = useParams();
  const [app, setApp] = useState<AppInfo | undefined>(undefined);

  useEffect(() => {
    const myApp = myApps.local.find((a) => appId(a) === params.id);
    if (myApp) return setApp(myApp);

    if (params.id) {
      const app = listedApps.find((a) => appId(a) === params.id);
      if (app) {
        setApp(app);
      } else {
        getListedApp(params.id)
          .then((app) => setApp(app))
          .catch(console.error);
      }
    }
  }, [params.id]);

  const goToPublish = useCallback(() => {
    navigate("/publish", { state: { app } });
  }, [app, navigate]);

  const version = useMemo(
    () => app?.metadata?.properties?.current_version || "Unknown",
    [app]
  );
  const versions = Object.entries(app?.metadata?.properties?.code_hashes || {});
  const hash =
    app?.state?.our_version ||
    (versions[(versions.length || 1) - 1] || ["", ""])[1];

  return (
    <div style={{ width: "100%" }}>
      <SearchHeader value="" onChange={() => null} hideSearch />
      <div className="card" style={{ marginTop: "1em" }}>
        {app ? (
          <>
            <div className="row between">
              <AppHeader app={app} size="large" />
              <ActionButton app={app} style={{ marginRight: "0.5em" }} />
            </div>
            <div className="col" style={{ marginTop: "1em" }}>
              <div className="app-details row">
                <div className="title">Description</div>
                <div className="value">
                  {(app.metadata?.description || "No description given").slice(
                    0,
                    2000
                  )}
                </div>
              </div>
              <div className="app-details row">
                <div className="title">Publisher</div>
                <div className="value underline">{app.publisher}</div>
              </div>
              <div className="app-details row">
                <div className="title">Version</div>
                <div className="value">{version}</div>
              </div>
              <div className="app-details row">
                <div className="title">Mirrors</div>
                <div className="col">
                  {(app.metadata?.properties?.mirrors || []).map(
                    (mirror, index) => (
                      <div key={index + mirror} className="value underline">
                        {mirror}
                      </div>
                    )
                  )}
                </div>
              </div>
              {/* <div className="app-details row">
                <div className="title">Permissions</div>
                <div className="col">
                  {app.permissions?.map((permission, index) => (
                    <div key={index + permission} className="value permission">{permission}</div>
                  ))}
                </div>
              </div> */}
              <div className="app-details row">
                <div className="title">Hash</div>
                <div className="value" style={{ wordBreak: "break-all" }}>
                  {hash}
                </div>
              </div>
            </div>
            <div className="app-screenshots row">
              {(app.metadata?.properties?.screenshots || []).map(
                (screenshot, index) => (
                  <img key={index + screenshot} src={screenshot} />
                )
              )}
            </div>
            {app.installed && (
              <button type="button" onClick={goToPublish}>
                Publish
              </button>
            )}
          </>
        ) : (
          <>
            <h4>App details not found for </h4>
            <h4>{params.id}</h4>
          </>
        )}
      </div>
    </div>
  );
}
