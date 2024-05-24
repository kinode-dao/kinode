import React, { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import Modal from "./Modal";
import { getAppName } from "../utils/app";
import Loader from "./Loader";
import classNames from "classnames";
import { FaDownload } from "react-icons/fa6";

interface DownloadButtonProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
  isIcon?: boolean;
}

export default function DownloadButton({ app, isIcon = false, ...props }: DownloadButtonProps) {
  const { downloadApp, getCaps, getMyApp, getMyApps } =
    useAppsStore();
  const [showModal, setShowModal] = useState(false);
  const [mirror, setMirror] = useState(app.metadata?.properties?.mirrors?.[0] || "Other");
  const [customMirror, setCustomMirror] = useState("");
  const [loading, setLoading] = useState("");

  useEffect(() => {
    setMirror(app.metadata?.properties?.mirrors?.[0] || "Other");
  }, [app.metadata?.properties?.mirrors]);

  const onClick = useCallback(async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    setShowModal(true);
  }, [app, setShowModal, getCaps]);

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
            getMyApps();
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

  const appName = getAppName(app);

  return (
    <>
      <button
        {...props}
        type="button"
        className={classNames("text-sm self-start", props.className, {
          'icon clear': isIcon,
          'black': !isIcon,
        })}
        onClick={onClick}
      >
        {isIcon ? <FaDownload /> : 'Download'}
      </button>
      <Modal show={showModal} hide={() => setShowModal(false)}>
        {loading ? (
          <Loader msg={loading} />
        ) : (
          <form className="flex flex-col items-center gap-2" onSubmit={download}>
            <h4>Download '{appName}'</h4>
            <h5>Select Mirror</h5>
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
        )}
      </Modal>
    </>
  );
}
