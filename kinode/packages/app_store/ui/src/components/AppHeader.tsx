import React from "react";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import AppIconPlaceholder from './AppIconPlaceholder'

interface AppHeaderProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
  size?: "small" | "medium" | "large";
  overrideImageSize?: "small" | "medium" | "large"
}

export default function AppHeader({
  app,
  size = "medium",
  overrideImageSize,
  ...props
}: AppHeaderProps) {
  const imageSize = overrideImageSize || size;

  return (
    <div {...props} className={`app-header app-header-${size} ${props.className || ""}`}>
      {size === 'small' && <AppName app={app} size={size} />}
      <AppIcon app={app} size={imageSize} />
      <div className={`app-info app-info-${size}`}>
        {size !== 'small' && <AppName app={app} size={size} />}
        {app.metadata?.description && (
          <AppDescription description={app.metadata.description} size={size} />
        )}
      </div>
    </div>
  );
}

function AppName({ app, size }: { app: AppInfo; size: string }) {
  return (
    <div className={`app-name app-name-${size}`}>
      {app.metadata?.name || appId(app)}
    </div>
  );
}

function AppIcon({ app, size }: { app: AppInfo; size: string }) {
  if (app.metadata?.image) {
    return (
      <img
        src={app.metadata.image}
        alt="app icon"
        className={`app-icon app-icon-${size}`}
      />
    );
  }
  return (
    <AppIconPlaceholder
      text={app.metadata_hash || app.state?.our_version?.toString() || ''}
      size={size as any}
    />
  );
}

function AppDescription({ description, size }: { description: string; size: string }) {
  return (
    <div className={`app-description app-description-${size}`}>
      {description}
    </div>
  );
}