import React, { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import Modal from "./Modal";
import { getAppName } from "../utils/app";
import Loader from "./Loader";
import classNames from "classnames";
import { FaU } from "react-icons/fa6";

interface UpdateButtonProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
  isIcon?: boolean;
}

export default function UpdateButton({ app, isIcon = false, ...props }: UpdateButtonProps) {
  const { updateApp, getCaps, getMyApp, getMyApps } =
    useAppsStore();
  const [showModal, setShowModal] = useState(false);
  const [caps, setCaps] = useState<string[]>([]);
  const [loading, setLoading] = useState("");


  const onClick = useCallback(async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    getCaps(app).then((manifest) => {
      setCaps(manifest.request_capabilities);
    });
    setShowModal(true);
  }, [app, setShowModal, getCaps]);

  const update = useCallback(async () => {
    try {
      setLoading(`Updating ${getAppName(app)}...`);
      await updateApp(app);

      const interval = setInterval(() => {
        getMyApp(app)
          .then((app) => {
            if (!app.installed) return;
            setLoading("");
            setShowModal(false);
            clearInterval(interval);
            getMyApps();
          })
          .catch(console.log);
      }, 2000);
    } catch (e) {
      console.error(e);
      window.alert(`Failed to update, please try again.`);
      setLoading("");
    }
  }, [app, updateApp, getMyApp]);

  return (
    <>
      <button
        {...props}
        type="button"
        className={classNames("text-sm self-start", props.className, {
          'icon clear': isIcon
        })}
        onClick={onClick}
      >
        {isIcon ? <FaU /> : 'Update'}
      </button>
      <Modal show={showModal} hide={() => setShowModal(false)}>
        {loading ? (
          <Loader msg={loading} />
        ) : (
          <>
            <h4>Approve App Permissions</h4>
            <h5 className="m-0">
              {getAppName(app)} needs the following permissions:
            </h5>
            {/* <h5>Send Messages:</h5> */}
            <br />
            <ul className="flex flex-col items-start">
              {caps.map((cap) => (
                <li key={cap}>{cap}</li>
              ))}
            </ul>
            {/* <h5>Receive Messages:</h5>
            <ul>
              {caps.map((cap) => (
                <li key={cap}>{cap}</li>
              ))}
            </ul> */}
            <button type="button" onClick={update}>
              Approve & Update
            </button>
          </>
        )}
      </Modal>
    </>
  );
}
