import React from "react";
import { useNavigate } from "react-router-dom";
import AppHeader from "./AppHeader";
import ActionButton from "./ActionButton";
import MoreActions from "./MoreActions";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import { APP_DETAILS_PATH } from "../constants/path";

interface AppEntryProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
  size?: "small" | "medium" | "large";
  overrideImageSize?: "small" | "medium" | "large";
  showMoreActions?: boolean;
  launchPath?: string;
}

export default function AppEntry({
  app,
  size = "medium",
  overrideImageSize,
  showMoreActions,
  launchPath,
  ...props
}: AppEntryProps) {
  const navigate = useNavigate();

  const handleClick = () => {
    if (!showMoreActions) {
      navigate(`/${APP_DETAILS_PATH}/${appId(app)}`);
    }
  };

  return (
    <div
      {...props}
      key={appId(app)}
      className={`app-entry app-entry-${size} ${props.className || ""}`}
      onClick={handleClick}
    >
      <AppHeader app={app} size={size} overrideImageSize={overrideImageSize} />
      <div className={`app-entry-actions app-entry-actions-${size}`}>
        <ActionButton
          app={app}
          launchPath={launchPath}
          isIcon={!showMoreActions && size !== "large"}
          className={`action-button-${size}`}
        />
        {showMoreActions && (
          <MoreActions app={app} className="more-actions" />
        )}
      </div>
    </div>
  );
}