import React, { useEffect, useMemo, useState } from "react";
import { AppInfo } from "../types/Apps";
import UpdateButton from "./UpdateButton";
import DownloadButton from "./DownloadButton";
import InstallButton from "./InstallButton";
import LaunchButton from "./LaunchButton";
import { FaCheck } from "react-icons/fa6";

interface ActionButtonProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
  isIcon?: boolean;
}

export default function ActionButton({ app, isIcon = false, ...props }: ActionButtonProps) {
  const [incrementNumber, setIncrementNumber] = useState(0);
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
  }, [app, incrementNumber]);


  const [launchPath, setLaunchPath] = useState('');

  useEffect(() => {
    fetch('/apps').then(data => data.json())
      .then((data: Array<{ package_name: string, path: string }>) => {
        if (Array.isArray(data)) {
          const homepageAppData = data.find(otherApp => app.package === otherApp.package_name)
          if (homepageAppData) {
            setLaunchPath(homepageAppData.path)
          }
        }
      })
  }, [app, incrementNumber])

  return (
    <>
      {(installed && launchPath)
        ? <LaunchButton app={app} {...props} isIcon={isIcon} launchPath={launchPath} />
        : (installed && updatable)
          ? <UpdateButton app={app} {...props} isIcon={isIcon} callback={() => setIncrementNumber(incrementNumber + 1)} />
          : !downloaded
            ? <DownloadButton app={app} {...props} isIcon={isIcon} callback={() => setIncrementNumber(incrementNumber + 1)} />
            : !installed
              ? <InstallButton app={app} {...props} isIcon={isIcon} callback={() => setIncrementNumber(incrementNumber + 1)} />
              : isIcon
                ? <button
                  className="pointer-events none icon clear absolute top-0 right-0"
                >
                  <FaCheck />
                </button>
                : <div>Installed</div>}
    </>
  );
}
