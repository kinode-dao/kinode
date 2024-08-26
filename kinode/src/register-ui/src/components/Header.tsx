import React from 'react';
import { ConnectButton } from '@rainbow-me/rainbowkit';

const Header: React.FC = () => {
    return (
        <header className="header">
            <div className="connect-wallet">
                <ConnectButton />
            </div>
        </header>
    );
};

export default Header;