import React from 'react';
import { Link, useLocation } from 'react-router-dom';
import { STORE_PATH, MY_APPS_PATH, PUBLISH_PATH } from '../constants/path';
import { ConnectButton } from '@rainbow-me/rainbowkit';

const Header: React.FC = () => {
    const location = useLocation();

    return (
        <header className="app-header">
            <div className="header-left">
                <nav>
                    <Link to={STORE_PATH} className={location.pathname === STORE_PATH ? 'active' : ''}>Home</Link>
                    <Link to={MY_APPS_PATH} className={location.pathname === MY_APPS_PATH ? 'active' : ''}>My Apps</Link>
                    <Link to={PUBLISH_PATH} className={location.pathname === PUBLISH_PATH ? 'active' : ''}>Publish</Link>
                </nav>
            </div>
            <div className="header-right">
                <ConnectButton />
            </div>
        </header>
    );
};

export default Header;