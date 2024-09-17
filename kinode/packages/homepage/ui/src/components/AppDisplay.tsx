import { HomepageApp } from "../store/homepageStore";

interface AppDisplayProps {
  app?: HomepageApp;
}

const AppDisplay: React.FC<AppDisplayProps> = ({ app }) => {
  return (
    <a
      id={app?.package_name}
      href={app?.path || undefined}
      className="app-display"
      title={app?.label}
      style={
        !app?.path
          ? {
              pointerEvents: "none",
              textDecoration: "none !important",
              filter: "grayscale(100%)",
            }
          : {}
      }
    >
      {app?.base64_icon ? (
        <img className="app-icon" src={app.base64_icon} />
      ) : (
        <img className="app-icon" src="/bird-orange.svg" />
      )}
      <h6 id="app-name">{app?.label || app?.package_name}</h6>
    </a>
  );
};

export default AppDisplay;
