import React, { useEffect, useMemo, useState } from "react";
import { AppInfo } from "../types/Apps";
import UpdateButton from "./UpdateButton";
import DownloadButton from "./DownloadButton";
import InstallButton from "./InstallButton";
import LaunchButton from "./LaunchButton";
import { FaCheck } from "react-icons/fa6";
import classNames from "classnames";

interface ActionButtonProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
  isIcon?: boolean;
  permitMultiButton?: boolean;
  launchPath?: string
}

export default function ActionButton({ app, launchPath = '', isIcon = false, permitMultiButton = false, ...props }: ActionButtonProps) {
  const { installed, downloaded, updatable } = useMemo(() => {
    const versions = Object.entries(app?.metadata?.properties?.code_hashes || {});
    const latestHash = (versions.find(([v]) => v === app.metadata?.properties?.current_version) || [])[1];

    const installed = app.installed;
    const downloaded = Boolean(app.state);

    const updatable =
      Boolean(app.state?.our_version && latestHash) &&
      app.state?.our_version !== latestHash &&
      app.publisher !== (window as any).our.node;
    return {
      installed,
      downloaded,
      updatable,
    };
  }, [app]);

  return (
    <>
      {/* if it's got a UI and it's updatable, show both buttons if we have space (launch will otherwise push out update) */}
      {permitMultiButton && installed && updatable && launchPath && <UpdateButton app={app} {...props} isIcon={isIcon} />}
      {(installed && launchPath)
        ? <LaunchButton app={app} {...props} isIcon={isIcon} launchPath={launchPath} />
        : (installed && updatable)
          ? <UpdateButton app={app} {...props} isIcon={isIcon} />
          : !downloaded
            ? <DownloadButton app={app} {...props} isIcon={isIcon} />
            : !installed
              ? <InstallButton app={app} {...props} isIcon={isIcon} />
              : isIcon
                ? <button
                  className="pointer-events none icon clear absolute top-0 right-0"
                >
                  <FaCheck />
                </button>
                : <></>
        // <button
        //   onClick={() => { }}
        //   {...props as any}
        //   className={classNames("clear pointer-events-none", props.className)}
        // >
        //   Installed
        // </button>
      }
    </>
  );
}
