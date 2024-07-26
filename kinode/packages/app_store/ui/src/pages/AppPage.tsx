import React, { useState, useEffect } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { FaGlobe, FaPeopleGroup, FaCode, FaLink, FaCaretDown } from "react-icons/fa6";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store";
import { PUBLISH_PATH } from "../constants/path";
import { FaCheckCircle, FaTimesCircle } from "react-icons/fa";

export default function AppPage() {
  const { getApp, installApp, updateApp, uninstallApp, setMirroring, setAutoUpdate } = useAppsStore();
  const navigate = useNavigate();
  const { id } = useParams();
  const [app, setApp] = useState<AppInfo | undefined>(undefined);

  useEffect(() => {
    if (id) {
      getApp(id).then(setApp).catch(console.error);
    }
  }, [id, getApp]);

  const handleInstall = () => app && installApp(app);
  const handleUpdate = () => app && updateApp(app);
  const handleUninstall = () => app && uninstallApp(app);
  const handleMirror = () => app && setMirroring(app, !app.state?.mirroring);
  const handleAutoUpdate = () => app && setAutoUpdate(app, !app.state?.auto_update);
  const handlePublish = () => navigate(PUBLISH_PATH, { state: { app } });

  if (!app) {
    return <div className="app-page"><h4>App details not found for {id}</h4></div>;
  }

  const version = app.metadata?.properties?.current_version || "Unknown";
  const versions = Object.entries(app.metadata?.properties?.code_hashes || {});
  const hash = app.state?.our_version || (versions[(versions.length || 1) - 1] || ["", ""])[1];
  const mirrors = app.metadata?.properties?.mirrors || [];

  const tbaLink = `https://optimistic.etherscan.io/address/${app.tba}`;

  return (
    <div className="app-page">
      <div className="app-details-card">
        <h2>{app.metadata?.name || app.package}</h2>
        <p>{app.metadata?.description || "No description available"}</p>
        <hr className="app-divider" />
        <div className="app-info-grid">
          <div className="app-info-item">
            <FaPeopleGroup size={36} />
            <span>Developer</span>
            <span>{app.publisher}</span>
          </div>
          <div className="app-info-item">
            <FaCode size={36} />
            <span>Version</span>
            <span>{version}</span>
            <span className="hash">{`${hash.slice(0, 5)}...${hash.slice(-5)}`}</span>
          </div>
          <div className="app-info-item">
            <FaGlobe size={36} />
            <span>Mirrors</span>
            <span>{mirrors.length}</span>
            <MirrorsDropdown mirrors={mirrors} />
          </div>
          <div className="app-info-item">
            <FaLink size={36} />
            <span>TBA</span>
            <a href={tbaLink} target="_blank" rel="noopener noreferrer">
              {`${app.package}.${app.publisher}`}
            </a>
          </div>
          <div className="app-info-item">
            <span>Installed</span>
            {app.installed ? <FaCheckCircle color="green" /> : <FaTimesCircle color="red" />}
          </div>
          {app.state && (
            <>
              <div className="app-info-item">
                <span>Mirrored From</span>
                <span>{app.state.mirrored_from || "N/A"}</span>
              </div>
              <div className="app-info-item">
                <span>Capabilities Approved</span>
                {app.state.caps_approved ? <FaCheckCircle color="green" /> : <FaTimesCircle color="red" />}
              </div>
              <div className="app-info-item">
                <span>Mirroring</span>
                {app.state.mirroring ? <FaCheckCircle color="green" /> : <FaTimesCircle color="red" />}
              </div>
              <div className="app-info-item">
                <span>Auto Update</span>
                {app.state.auto_update ? <FaCheckCircle color="green" /> : <FaTimesCircle color="red" />}
              </div>
              <div className="app-info-item">
                <span>Verified</span>
                {app.state.verified ? <FaCheckCircle color="green" /> : <FaTimesCircle color="red" />}
              </div>
            </>
          )}
        </div>
        {app.metadata?.properties?.screenshots && (
          <div className="app-screenshots">
            {app.metadata.properties.screenshots.map((screenshot, index) => (
              <img key={index} src={screenshot} alt={`Screenshot ${index + 1}`} className="app-screenshot" />
            ))}
          </div>
        )}
        <div className="app-actions">
          {app.installed ? (
            <>
              <button onClick={handleUpdate}>Update</button>
              <button onClick={handleUninstall}>Uninstall</button>
              <button onClick={handleMirror}>
                {app.state?.mirroring ? "Stop Mirroring" : "Start Mirroring"}
              </button>
              <button onClick={handleAutoUpdate}>
                {app.state?.auto_update ? "Disable Auto-Update" : "Enable Auto-Update"}
              </button>
              {app.state?.mirroring && (
                <button onClick={handlePublish}>Publish</button>
              )}
            </>
          ) : (
            <button onClick={handleInstall}>Install</button>
          )}
        </div>
      </div>
    </div>
  );
}

const MirrorsDropdown = ({ mirrors }: { mirrors: string[] }) => {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <div className="mirrors-dropdown">
      <button onClick={() => setIsOpen(!isOpen)} className="mirrors-dropdown-toggle">
        Mirrors <FaCaretDown />
      </button>
      {isOpen && (
        <ul className="mirrors-list">
          {mirrors.map((mirror, index) => (
            <li key={index}>{mirror}</li>
          ))}
        </ul>
      )}
    </div>
  );
};