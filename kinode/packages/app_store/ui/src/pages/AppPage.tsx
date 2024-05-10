import React, { useState, useEffect, useMemo, useCallback, ReactElement } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import ActionButton from "../components/ActionButton";
import AppHeader from "../components/AppHeader";
import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { appId } from "../utils/app";
import { PUBLISH_PATH } from "../constants/path";
import HomeButton from "../components/HomeButton";
import classNames from "classnames";
import { isMobileCheck } from "../utils/dimensions";
import { FaGlobe, FaPeopleGroup, FaStar } from "react-icons/fa6";

interface AppPageProps extends PageProps { }

export default function AppPage() {
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
    navigate(PUBLISH_PATH, { state: { app } });
  }, [app, navigate]);

  const version = useMemo(
    () => app?.metadata?.properties?.current_version || "Unknown",
    [app]
  );
  const versions = Object.entries(app?.metadata?.properties?.code_hashes || {});
  const hash =
    app?.state?.our_version ||
    (versions[(versions.length || 1) - 1] || ["", ""])[1];

  const isMobile = isMobileCheck()

  const appDetails: Array<{ top: ReactElement, middle: ReactElement, bottom: ReactElement }> = [
    {
      top: <div>0 ratings</div>,
      middle: <span className="text-2xl">5.0</span>,
      bottom: <div className="flex-center gap-1">
        <FaStar />
        <FaStar />
        <FaStar />
        <FaStar />
        <FaStar />
      </div>
    },
    {
      top: <div>Developer</div>,
      middle: <FaPeopleGroup size={36} />,
      bottom: <div>
        {app?.publisher}
      </div>
    },
    {
      top: <div>Version</div>,
      middle: <span className="text-2xl">{version}</span>,
      bottom: <div>
        {hash.slice(0, 5)}...{hash.slice(-5)}
      </div>
    },
    {
      top: <div>Mirrors</div>,
      middle: <FaGlobe size={36} />,
      bottom: <div>
        {app?.metadata?.properties?.mirrors?.length || 0}
      </div>
    }
  ]

  return (
    <div className={classNames("flex flex-col w-full max-w-[900px]",
      {
        'gap-4': isMobile,
        'gap-8': !isMobile,
      })}
    >
      <HomeButton />
      <SearchHeader
        value=""
        onChange={() => null}
        hideSearch
        hidePublish
      />
      <div className={classNames("flex-col-center card !rounded-3xl", {
        'p-12 gap-4': isMobile,
        'p-24 gap-8': !isMobile,
      })}>
        {app ? <>
          <AppHeader app={app} size="large" />
          <div className="w-5/6 h-0 border border-orange" />
          <div className={classNames("flex items-start text-xl", {
            'gap-4': isMobile,
            'gap-8': !isMobile,
          })}>
            {appDetails.map((detail, index) => <>
              <div
                className="flex-col-center gap-2 justify-between opacity-50 self-stretch"
                key={index}
              >
                {detail.top}
                {detail.middle}
                {detail.bottom}
              </div>
              {index !== appDetails.length - 1 && <div className="h-3/4 w-0 border border-orange self-center" />}
            </>)}
          </div>
          {/* <div className="flex flex-col mt-2">
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
            <div className="flex mt-1 items-start">
              <div className="w-1/4">Hash</div>
              <div className="w-3/4 break-all">
                {hash}
              </div>
            </div>
          </div> */}
          {Array.isArray(app.metadata?.properties?.screenshots)
            && app.metadata?.properties.screenshots.length > 0
            && <div className="flex overflow-x-auto max-w-full">
              {app.metadata.properties.screenshots.map(
                (screenshot, index) => (
                  <img key={index + screenshot} src={screenshot} className="mr-2 max-h-20 max-w-full rounded border border-black" />
                )
              )}
            </div>}
          <ActionButton app={app} className={classNames("self-center bg-orange text-lg px-12")} />
          {app.installed && app.state?.mirroring && (
            <button type="button" onClick={goToPublish}>
              Publish
            </button>
          )}
        </> : <>
          <h4>App details not found for </h4>
          <h4>{params.id}</h4>
        </>}
      </div>
    </div>
  );
}
