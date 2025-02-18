import React from 'react';
import { Link, useLocation } from 'react-router-dom';
import { STORE_PATH, PUBLISH_PATH, MY_APPS_PATH } from '../constants/path';
import { ConnectButton } from '@rainbow-me/rainbowkit';
import { FaHome } from "react-icons/fa";
import NotificationBay from './NotificationBay';
import useAppsStore from '../store';

const Header: React.FC = () => {
    const location = useLocation();
    const { updates } = useAppsStore();
    const updateCount = Object.keys(updates || {}).length;

    return (
        <header className="app-header">
            <div className="header-left">
                <nav>
                    <button onClick={() => window.location.href = window.location.origin.replace('//app-store-sys.', '//') + '/'} className="home-button">
                        <FaHome />
                    </button>
                    <Link to={STORE_PATH} className={location.pathname === STORE_PATH ? 'active' : ''}>Store</Link>
                    <Link to={MY_APPS_PATH} className={location.pathname === MY_APPS_PATH ? 'active' : ''}>
                        My Apps
                        {updateCount > 0 && <span className="update-badge">{updateCount}</span>}
                    </Link>
                    <Link to={PUBLISH_PATH} className={location.pathname === PUBLISH_PATH ? 'active' : ''}>Publish</Link>
                </nav>
            </div>
            <div className="header-right">
                <NotificationBay />
                <ConnectButton />
            </div>
        </header>
    );
};

export default Header;