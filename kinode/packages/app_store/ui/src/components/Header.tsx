import React from 'react';
import { Link } from 'react-router-dom';
import { STORE_PATH, PUBLISH_PATH, MY_APPS_PATH } from '../constants/path';
import { ConnectButton } from '@rainbow-me/rainbowkit';
import { FaHome } from "react-icons/fa";

const Header: React.FC = () => {
    return (
        <header className="app-header">
            <div className="header-left">
                <nav>
                    <button onClick={() => window.location.href = window.location.origin.replace('//app-store-sys.', '//') + '/'} className="home-button">
                        <FaHome />
                    </button>
                    <Link to={STORE_PATH} className={location.pathname === STORE_PATH ? 'active' : ''}>Apps</Link>
                    <Link to={PUBLISH_PATH} className={location.pathname === PUBLISH_PATH ? 'active' : ''}>Publish</Link>
                    <Link to={MY_APPS_PATH} className={location.pathname === MY_APPS_PATH ? 'active' : ''}>My Apps</Link>
                </nav>
            </div>
            <div className="header-right">
                <ConnectButton />
            </div>
        </header>
    );
};

export default Header;