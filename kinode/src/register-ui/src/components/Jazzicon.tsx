import React, { useEffect, useRef } from 'react';
import jazzicon from 'jazzicon';

interface JazziconProps extends React.HTMLAttributes<HTMLDivElement> {
    address: string;
    diameter?: number;
}

const Jazzicon: React.FC<JazziconProps> = ({ address, diameter = 40, ...props }) => {
    const ref = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (address && ref.current) {
            const seed = parseInt(address.slice(2, 10), 16); // Derive a seed from Ethereum address
            const icon = jazzicon(diameter, seed);

            // Clear the current icon
            ref.current.innerHTML = '';
            // Append the new icon
            ref.current.appendChild(icon);
        }
    }, [address, diameter]);

    return <div {...props} ref={ref} />;
};

export default Jazzicon;
