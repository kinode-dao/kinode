import React, { useState, useEffect } from "react";
import useHomepageStore from "../store/homepageStore";
import AppDisplay from "./AppDisplay";

const AllApps: React.FC = () => {
  const { apps } = useHomepageStore();
  const [expanded, setExpanded] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const [visibleApps, setVisibleApps] = useState(5);

  useEffect(() => {
    const handleResize = () => {
      const mobile = window.innerWidth <= 768;
      setIsMobile(mobile);
      if (mobile) {
        setVisibleApps(0);
      } else if (window.innerWidth <= 1024) {
        setVisibleApps(3);
      } else {
        setVisibleApps(5);
      }
    };

    handleResize();
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  const displayedApps = expanded ? apps : apps.slice(0, visibleApps);
  const hasMoreApps = apps.length > visibleApps;

  const handleExpandClick = () => {
    setExpanded(!expanded);
  };

  return (
    <div id="all-apps" className={isMobile ? "mobile" : ""}>
      <div
        className={`apps-grid ${expanded ? "expanded" : ""} ${
          isMobile ? "mobile" : ""
        }`}
      >
        {displayedApps.map((app) => (
          <AppDisplay key={app.id} app={app} />
        ))}
      </div>
      {(hasMoreApps || isMobile) && (
        <button className="expand-button" onClick={handleExpandClick}>
          {expanded
            ? "Hide Apps"
            : `Show ${isMobile ? "Apps" : `All (${apps.length})`}`}
        </button>
      )}
    </div>
  );
};

export default AllApps;
