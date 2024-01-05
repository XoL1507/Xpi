// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import cl from 'classnames';
import { Link } from 'react-router-dom';

import Icon, { SuiIcons } from '_components/icon';

import st from './Select.module.scss';

const selections = [
    {
        title: 'Yes, let’s get set up!',
        desc: 'This will create a new wallet and a seed phrase.',
        url: '../create',
        action: 'Create new wallet',
        icon: SuiIcons.Plus,
    },
    {
        title: 'No, I already have one',
        desc: 'Import your existing wallet by entering the 12-word seed phrase',
        url: '../import',
        action: 'Import a wallet',
        icon: SuiIcons.Download,
    },
];

const SelectPage = () => {
    return (
        <>
            <h1 className={st.headerTitle}>New to Sui Wallet?</h1>
            <div className={st.selector}>
                {selections.map((aSelection) => (
                    <div className={st.card} key={aSelection.url}>
                        <h3 className={st.title}>{aSelection.title}</h3>
                        <div className={st.desc}>{aSelection.desc}</div>
                        <Link
                            to={aSelection.url}
                            className={cl('btn', st.action)}
                        >
                            <Icon icon={aSelection.icon} className={st.icon} />
                            {aSelection.action}
                        </Link>
                    </div>
                ))}
            </div>
        </>
    );
};

export default SelectPage;
