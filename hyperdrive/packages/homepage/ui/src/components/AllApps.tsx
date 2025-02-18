import React, { useState, useEffect, useMemo } from "react";
import useHomepageStore, { HomepageApp } from "../store/homepageStore";
import AppDisplay from "./AppDisplay";

const AllApps: React.FC = () => {
  const { apps } = useHomepageStore();
  const [expanded, setExpanded] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const [visibleApps, setVisibleApps] = useState(5);
  const [orderedApps, setOrderedApps] = useState<HomepageApp[]>([]);
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);

  useEffect(() => {
    const handleResize = () => {
      const mobile = window.innerWidth <= 768;
      setIsMobile(mobile);
      setVisibleApps(mobile ? 0 : window.innerWidth <= 1024 ? 3 : 5);
    };

    handleResize();
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  // Sort apps based on persisted order
  const sortedApps = useMemo(() => {
    // Use orderedApps if we have them, otherwise fall back to apps
    const appsToSort = orderedApps.length ? orderedApps : apps;
    return [...appsToSort]
      .sort((a, b) => a.order - b.order)
      .filter(app => app.path !== null);
  }, [apps, orderedApps]);

  const displayedApps = expanded
    ? sortedApps
    : sortedApps.slice(0, visibleApps);
  const hasMoreApps = sortedApps.length > visibleApps;

  const handleExpandClick = () => {
    setExpanded(!expanded);
  };

  const handleDragStart = (e: React.DragEvent, index: number) => {
    e.dataTransfer.setData("text/plain", index.toString());
    setDraggedIndex(index);
  };

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault();
    setDragOverIndex(index);
  };

  const handleDragEnd = () => {
    setDraggedIndex(null);
    setDragOverIndex(null);
  };

  const handleDrop = (e: React.DragEvent, dropIndex: number) => {
    e.preventDefault();
    const dragIndex = parseInt(e.dataTransfer.getData("text/plain"), 10);
    if (dragIndex === dropIndex) return;

    const newSortedApps = [...sortedApps];
    const [movedApp] = newSortedApps.splice(dragIndex, 1);
    newSortedApps.splice(dropIndex, 0, movedApp);

    const updatedApps = newSortedApps.map((app, index) => ({
      ...app,
      order: index
    }));

    setOrderedApps(updatedApps);

    handleDragEnd();

    // Sync the order with the backend
    fetch('/order', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      credentials: 'include',
      body: JSON.stringify(newSortedApps.map((app, index) => [app.id, index]))
    });
  };

  return (
    <div id="all-apps" className={isMobile ? "mobile" : ""}>
      <div
        className={`apps-grid ${expanded ? "expanded" : ""} ${isMobile ? "mobile" : ""
          }`}
        style={{ gridTemplateColumns: `repeat(${Math.min(displayedApps.length, 5)}, 1fr)` }}
      >
        {displayedApps.map((app, index) => (
          <div
            key={`${app.id}-${app.order}`}
            draggable
            onDragStart={(e) => handleDragStart(e, index)}
            onDragOver={(e) => handleDragOver(e, index)}
            onDragEnd={handleDragEnd}
            onDrop={(e) => handleDrop(e, index)}
            className={`app-wrapper ${draggedIndex === index ? "dragging" : ""
              } ${dragOverIndex === index ? "drag-over" : ""}`}
          >
            <AppDisplay app={app} />
            <div className="drag-handle">⋮⋮</div>
          </div>
        ))}
      </div>
      {(hasMoreApps || isMobile) && (
        <button className="expand-button" onClick={handleExpandClick}>
          {expanded
            ? "Hide Apps"
            : `Show ${isMobile ? "Apps" : `All (${sortedApps.length})`}`}
        </button>
      )}
    </div>
  );
};

export default AllApps;
