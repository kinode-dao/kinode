import React, { useState, useEffect, useMemo, useCallback } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import ActionButton from "../components/ActionButton";
import AppHeader from "../components/AppHeader";
import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { appId } from "../utils/app";

interface AppPageProps extends PageProps { }

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
    <div className="flex flex-col w-full max-w-[900px]">
      <SearchHeader value="" onChange={() => null} hideSearch />
      <div className="card mt1">
        {app ? (
          <>
            <div className="flex justify-between">
              <AppHeader app={app} size="large" />
              <ActionButton app={app} className="mr-1" />
            </div>
            <div className="flex flex-col mt-2">
              <div className="flex mt-1 items-start">
                <div className="w-1/4">Description</div>
                <div className="mb-1 w-3/4">
                  {(app.metadata?.description || "No description given").slice(
                    0,
                    2000
                  )}
                </div>
              </div>
              <div className="flex mt-1 items-start">
                <div className="w-1/4">Publisher</div>
                <div className="mb-1 w-3/4">{app.publisher}</div>
              </div>
              <div className="flex mt-1 items-start">
                <div className="w-1/4">Version</div>
                <div className="mb-1 w-3/4">{version}</div>
              </div>
              <div className="flex mt-1 items-start">
                <div className="w-1/4">Mirrors</div>
                <div className="w-3/4 flex flex-col">
                  {(app.metadata?.properties?.mirrors || []).map(
                    (mirror, index) => (
                      <div key={index + mirror} className="mb-1">
                        {mirror}
                      </div>
                    )
                  )}
                </div>
              </div>
              {/* <div className="flex mt-1 items-start">
                <div className="w-1/4">Permissions</div>
                <div className="w-3/4 flex flex-col">
                  {app.permissions?.map((permission, index) => (
                    <div key={index + permission} className="mb-1">{permission}</div>
                  ))}
                </div>
              </div> */}
              <div className="flex mt-1 items-start">
                <div className="w-1/4">Hash</div>
                <div className="w-3/4 break-all">
                  {hash}
                </div>
              </div>
            </div>
            <div className="app-screenshots flex mt-2 overflow-x-auto max-w-full">
              {(app.metadata?.properties?.screenshots || []).map(
                (screenshot, index) => (
                  <img key={index + screenshot} src={screenshot} className="mr-2 max-h-20 max-w-full rounded border border-black" />
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
