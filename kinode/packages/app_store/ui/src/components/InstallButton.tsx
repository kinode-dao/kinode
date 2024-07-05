import React, { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import Modal from "./Modal";
import { getAppName } from "../utils/app";
import Loader from "./Loader";
import classNames from "classnames";
import { FaI } from "react-icons/fa6";

interface InstallButtonProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
  isIcon?: boolean;
}

export default function InstallButton({ app, isIcon = false, ...props }: InstallButtonProps) {
  const { installApp, getCaps, getMyApp, getMyApps } =
    useAppsStore();
  const [showModal, setShowModal] = useState(false);
  const [caps, setCaps] = useState<any[]>([]);
  const [installing, setInstalling] = useState("");

  const onClick = useCallback(async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    getCaps(app).then((manifest) => {
      setCaps(manifest.request_capabilities);
    });
    setShowModal(true);
  }, [app, setShowModal, getCaps]);

  const install = useCallback(async () => {
    try {
      setInstalling(`Installing ${getAppName(app)}...`);
      await installApp(app);

      const interval = setInterval(() => {
        getMyApp(app)
          .then((app) => {
            if (!app.installed) return;
            setInstalling("");
            setShowModal(false);
            clearInterval(interval);
            getMyApps();
          })
          .catch(console.log);
      }, 2000);
    } catch (e) {
      console.error(e);
      window.alert(`Failed to install, please try again.`);
      setInstalling("");
    }
  }, [app, installApp, getMyApp]);

  return (
    <>
      <button
        {...props}
        type="button"
        className={classNames("text-sm self-start", props.className, {
          'icon clear': isIcon
        })}
        onClick={onClick}
        disabled={!!installing}
      >
        {isIcon
          ? <FaI />
          : installing
            ? 'Installing...'
            : "Install"}
      </button>
      <Modal show={showModal} hide={() => setShowModal(false)}>
        {installing ? (
          <div className="flex-col-center gap-4">
            <Loader msg={installing} />
            <div className="text-center">
              App is installing in the background. You can safely close this window.
            </div>
          </div>
        ) : (
          <div className="flex-col-center gap-2">
            <h4>Approve App Permissions</h4>
            <h5 className="m-0">
              {getAppName(app)} needs the following permissions:
            </h5>
            <ul className="flex flex-col items-start">
              {caps.map((cap) => (
                <li>{JSON.stringify(cap)}</li>
              ))}
            </ul>
            <button type="button" onClick={install}>
              Approve & Install
            </button>
          </div>
        )}
      </Modal>
    </>
  );
}
