import React, { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import Modal from "./Modal";
import { getAppName } from "../utils/app";
import Loader from "./Loader";
import classNames from "classnames";

interface ActionButtonProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
}

export default function ActionButton({ app, ...props }: ActionButtonProps) {
  const { updateApp, downloadApp, installApp, getCaps, getMyApp } =
    useAppsStore();
  const [showModal, setShowModal] = useState(false);
  const [mirror, setMirror] = useState(app.metadata?.properties?.mirrors?.[0] || "Other");
  const [customMirror, setCustomMirror] = useState("");
  const [caps, setCaps] = useState<string[]>([]);
  const [loading, setLoading] = useState("");

  const { clean, installed, downloaded, updatable } = useMemo(() => {
    const versions = Object.entries(app?.metadata?.properties?.code_hashes || {});
    const latestHash = (versions.find(([v]) => v === app.metadata?.properties?.current_version) || [])[1];

    const installed = app.installed;
    const downloaded = Boolean(app.state);

    const updatable =
      Boolean(app.state?.our_version && latestHash) &&
      app.state?.our_version !== latestHash &&
      app.publisher !== window.our.node;
    return {
      clean: !installed && !downloaded && !updatable,
      installed,
      downloaded,
      updatable,
    };
  }, [app]);

  useEffect(() => {
    setMirror(app.metadata?.properties?.mirrors?.[0] || "Other");
  }, [app.metadata?.properties?.mirrors]);

  const onClick = useCallback(async () => {
    if (installed && !updatable) {
      window.alert("App is installed");
    } else {
      if (downloaded) {
        getCaps(app).then((manifest) => {
          setCaps(manifest.request_capabilities);
        });
      }
      setShowModal(true);
    }
  }, [app, installed, downloaded, updatable, setShowModal, getCaps]);

  const download = useCallback(async (e: FormEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const targetMirror = mirror === "Other" ? customMirror : mirror;

    if (!targetMirror) {
      window.alert("Please select a mirror");
      return;
    }

    try {
      setLoading(`Downloading ${getAppName(app)}...`);
      await downloadApp(app, targetMirror);
      const interval = setInterval(() => {
        getMyApp(app)
          .then(() => {
            setLoading("");
            setShowModal(false);
            clearInterval(interval);
          })
          .catch(console.log);
      }, 2000);
    } catch (e) {
      console.error(e);
      window.alert(
        `Failed to download app from ${targetMirror}, please try a different mirror.`
      );
      setLoading("");
    }
  }, [mirror, customMirror, app, downloadApp, getMyApp]);

  const install = useCallback(async () => {
    try {
      setLoading(`Installing ${getAppName(app)}...`);
      await installApp(app);

      const interval = setInterval(() => {
        getMyApp(app)
          .then((app) => {
            if (!app.installed) return;
            setLoading("");
            setShowModal(false);
            clearInterval(interval);
          })
          .catch(console.log);
      }, 2000);
    } catch (e) {
      console.error(e);
      window.alert(`Failed to install, please try again.`);
      setLoading("");
    }
  }, [app, installApp, getMyApp]);

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
          })
          .catch(console.log);
      }, 2000);
    } catch (e) {
      console.error(e);
      window.alert(`Failed to update, please try again.`);
      setLoading("");
    }
  }, [app, updateApp, getMyApp]);

  const appName = getAppName(app);

  return (
    <>
      <button
        {...props}
        type="button"
        className={classNames("text-sm min-w-[100px] px-2 py-1 self-start", props.className)}
        onClick={onClick}
      >
        {installed && updatable
          ? "Update"
          : installed
            ? "Installed"
            : downloaded
              ? "Install"
              : "Download"}
      </button>
      <Modal show={showModal} hide={() => setShowModal(false)}>
        {loading ? (
          <Loader msg={loading} />
        ) : clean ? (
          <form className="flex flex-col items-center gap-2" onSubmit={download}>
            <h4>Download '{appName}'</h4>
            <h5 style={{ margin: 0 }}>Select Mirror</h5>
            <select value={mirror} onChange={(e) => setMirror(e.target.value)}>
              {((app.metadata?.properties?.mirrors || []).concat(["Other"])).map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
            {mirror === "Other" && (
              <input
                type="text"
                value={customMirror}
                onChange={(e) => setCustomMirror(e.target.value)}
                placeholder="Mirror, i.e. 'template.os'"
                className="p-1 max-w-[240px] w-full"
                required
                autoFocus
              />
            )}
            <button type="submit">
              Download
            </button>
          </form>
        ) : downloaded ? (
          <>
            <h4>Approve App Permissions</h4>
            <h5 className="m-0">
              {getAppName(app)} needs the following permissions:
            </h5>
            <ul className="flex flex-col items-start">
              {caps.map((cap) => (
                <li key={cap}>{cap}</li>
              ))}
            </ul>
            <button type="button" onClick={install}>
              Approve & Install
            </button>
          </>
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
