import React from "react";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import { useNavigate } from "react-router-dom";

interface AppHeaderProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
  size?: "small" | "medium" | "large";
}

export default function AppHeader({
  app,
  size = "medium",
  ...props
}: AppHeaderProps) {
  const navigate = useNavigate()

  return (
    <div
      {...props}
      className={`app-header row ${size} ${props.className || ""}`}
      onClick={() => navigate(`/app-details/${appId(app)}`)}
    >
      <img
        src={
          app.metadata?.image ||
          "https://png.pngtree.com/png-vector/20190215/ourmid/pngtree-vector-question-mark-icon-png-image_515448.jpg"
        }
        alt="app icon"
      />
      <div className="col title">
        <div className="app-name ellipsis">{app.metadata?.name || appId(app)}</div>
        {app.metadata?.description && size !== "large" && (
          <div className="ellipsis">{app.metadata?.description?.slice(0, 100)}</div>
        )}
      </div>
    </div>
  );
}
