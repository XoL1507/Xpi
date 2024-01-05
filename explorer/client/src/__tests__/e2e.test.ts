// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import puppeteer, { type Page, type Browser } from 'puppeteer';

//Global values:
let browser: Browser;
let page: Page;
const BASE_URL = 'http://localhost:8080';

//Standardized CSS Selectors

const coinGroup = (num: number) => {
    const trunk = `#groupCollection > tr:nth-child(${num + 1})`;
    return {
        base: () => trunk,
        field: (numField: number) => `${trunk} > td:nth-child(${numField})`,
    };
};

const mainBodyCSS = 'main > section > div';

const nftObject = (num: number) => `div#ownedObjects > div:nth-child(${num}) a`;

//Standardized Expectations
const cssInteract = (page: Page) => ({
    with: (cssValue: string) => ({
        get: {
            attribute: async (attr: string): Promise<string> => {
                const result = await page.$eval(
                    cssValue,
                    (el, attr) => el.getAttribute(attr as string),
                    attr
                );
                return result === null ? '' : (result as string);
            },
            textContent: async (): Promise<string> => {
                const text = await page.$eval(cssValue, (el) => el.textContent);
                return text === null ? '' : (text as string);
            },
            isDisabled: async (): Promise<boolean> =>
                page.$eval(cssValue, (el) => el.hasAttribute('disabled')),
        },
        click: async (): Promise<void> =>
            page.$eval(cssValue, (el) => (el as HTMLElement).click()),
    }),
});

const expectHome = async (page: Page) => {
    const result = await cssInteract(page)
        .with(mainBodyCSS)
        .get.attribute('data-testid');
    expect(result).toBe('home-page');
};

const expectErrorResult = async (page: Page) => {
    const result = await cssInteract(page)
        .with(mainBodyCSS)
        .get.attribute('id');
    expect(result).toBe('errorResult');
};

const searchText = async (page: Page, text: string) => {
    await page.type('#searchText', text);
    await cssInteract(page).with('#searchBtn').click();
};

describe('End-to-end Tests', () => {
    beforeAll(async () => {
        browser = await puppeteer.launch();
        page = await browser.newPage();
    });

    afterAll(async () => {
        browser.close();
    });

    describe('The Home Page', () => {
        it('is the landing page', async () => {
            await page.goto(BASE_URL);
            await expectHome(page);
        });

        it('is the redirect page', async () => {
            await page.goto(`${BASE_URL}/apples`);
            await expectHome(page);
        });

        it('has a go home button', async () => {
            await page.goto(`${BASE_URL}/apples`);
            await cssInteract(page).with('#homeBtn').click();
            await expectHome(page);
        });
    });

    describe('Wrong Search', () => {
        it('leads to error page', async () => {
            await page.goto(BASE_URL);
            await searchText(page, 'apples');
            await expectErrorResult(page);
        });
    });

    describe('Object Results', () => {
        const successObjectID = 'CollectionObject';
        const problemObjectID = 'ProblemObject';

        it('can be searched', async () => {
            await page.goto(BASE_URL);
            await searchText(page, successObjectID);
            const value = await cssInteract(page)
                .with('#objectID')
                .get.textContent();
            expect(value.trim()).toBe(successObjectID);
        });

        it('can be reached through URL', async () => {
            await page.goto(BASE_URL);
            await page.goto(`${BASE_URL}/objects/${successObjectID}`);
            const value = await cssInteract(page)
                .with('#objectID')
                .get.textContent();
            expect(value.trim()).toBe(successObjectID);
        });

        it('can have missing data', async () => {
            await page.goto(`${BASE_URL}/objects/${problemObjectID}`);
            await expectErrorResult(page);
        });
    });

    describe('Address Results', () => {
        const successAddressID = 'receiverAddress';
        const noObjectsAddressID = 'senderAddress';
        it('can be searched', async () => {
            await page.goto(BASE_URL);
            await searchText(page, successAddressID);
            const value = await cssInteract(page)
                .with('#addressID')
                .get.textContent();
            expect(value.trim()).toBe(successAddressID);
        });

        it('can be reached through URL', async () => {
            await page.goto(`${BASE_URL}/addresses/${successAddressID}`);
            const value = await cssInteract(page)
                .with('#addressID')
                .get.textContent();
            expect(value.trim()).toBe(successAddressID);
        });
        it('displays error when no objects', async () => {
            await page.goto(`${BASE_URL}/objects/${noObjectsAddressID}`);
            await expectErrorResult(page);
        });
    });

    /*
    describe('Transaction Results', () => {
        // disabled because we are not do not display the Word Transaction ID
        const successID = 'Da4vHc9IwbvOYblE8LnrVsqXwryt2Kmms+xnJ7Zx5E4=';
        it('can be searched', async () => {
            await page.goto(BASE_URL);
            await searchText(page, successID);
            const value = await cssInteract(page)
                .with('#transactionID')
                .get.textContent();
            expect(value.trim()).toBe(successID);
        });

        it('can be reached through URL', async () => {
            await page.goto(`${BASE_URL}/transactions/${successID}`);
            const value = await cssInteract(page)
                .with('#transactionID')
                .get.textContent();
            expect(value.trim()).toBe(successID);
        });
        it('correctly renders days and hours', async () => {
            await page.goto(`${BASE_URL}/transactions/${successID}`);
            const value = await cssInteract(page)
                .with('#timestamp')
                .get.textContent();
            expect(value.trim()).toBe(
                '17 days 1 hour ago (15 Dec 2024 00:00:00 UTC)'
            );
        });
        it('correctly renders a time on the cusp of a year', async () => {
            const otherID = 'GHTP9gcFmF5KTspnz3KxXjvSH8Bx0jv68KFhdqfpdK8=';
            await page.goto(`${BASE_URL}/transactions/${otherID}`);
            const value = await cssInteract(page)
                .with('#timestamp')
                .get.textContent();
            expect(value.trim()).toBe(
                '1 min 3 secs ago (01 Jan 2025 01:12:07 UTC)'
            );
        });
        it('correctly renders a time diff of less than 1 sec', async () => {
            const otherID = 'XHTP9gcFmF5KTspnz3KxXjvSH8Bx0jv68KFhdqfpdK8=';
            await page.goto(`${BASE_URL}/transactions/${otherID}`);
            const value = await cssInteract(page)
                .with('#timestamp')
                .get.textContent();
            expect(value.trim()).toBe('< 1 sec ago (01 Jan 2025 01:13:09 UTC)');
        });
    });*/

    describe('Owned Objects have links that enable', () => {
        const navigationTemplate = async (
            page: Page,
            parentValue: string,
            parentIsA: 'addresses' | 'objects',
            childValue: string,
            parentToChildNo: number
        ) => {
            await page.goto(`${BASE_URL}/${parentIsA}/${parentValue}`);

            //Click on child in Owned Objects List:
            await cssInteract(page).with(nftObject(parentToChildNo)).click();

            //Check ID of child object:
            const childText = await cssInteract(page)
                .with('#objectID')
                .get.textContent();
            expect(childText.trim()).toBe(childValue);

            //Click on Owner text:
            await cssInteract(page)
                .with('div#owner > div >span:first-child')
                .click();

            //Looking for object or address?
            const lookingFor =
                parentIsA === 'addresses' ? '#addressID' : '#objectID';

            //Check ID of parent:
            const parentText = await cssInteract(page)
                .with(lookingFor)
                .get.textContent();
            expect(parentText.trim()).toBe(parentValue);
        };
        it('going from address to object and back', async () => {
            await navigationTemplate(
                page,
                'receiverAddress',
                'addresses',
                'player1',
                1
            );
        });
        it('going from object to child object and back', async () => {
            await navigationTemplate(page, 'player2', 'objects', 'Image1', 1);
        });
        it('going from parent to broken image object and back', async () => {
            const parentValue = 'ObjectWBrokenChild';
            await page.goto(`${BASE_URL}/objects/${parentValue}`);

            // 1) Click on child in Owned Objects List:
            await cssInteract(page).with(nftObject(1)).click();

            await page.waitForFunction(() =>
                document.querySelector('#noImage')
            );

            // 2) This leads to a no image warning:
            expect(
                await cssInteract(page).with('#noImage').get.textContent()
            ).toBe('No Image was Found');

            // 3) Click on owner:
            await page.click('div#owner span:first-child');

            await page.waitForFunction(() =>
                document.querySelector('#loadedImage')
            );

            // 4) This leads to an image:
            expect(
                await cssInteract(page)
                    .with('div#displayContainer > img')
                    .get.attribute('id')
            ).toBe('loadedImage');
        });
    });
    describe('PaginationWrapper has buttons', () => {
        it('to go to the next page', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);
            await cssInteract(page).with('#nextBtn').click();
            await cssInteract(page).with(nftObject(1)).click();
            const value = await cssInteract(page)
                .with('#objectID')
                .get.textContent();
            expect(value.trim()).toBe('Image2');
        });
        it('to go to the last page', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);
            await cssInteract(page).with('#lastBtn').click();
            await cssInteract(page).with(nftObject(1)).click();
            const value = await cssInteract(page)
                .with('#objectID')
                .get.textContent();
            expect(value.trim()).toBe('CollectionObject');
        });

        it('where last and next disappear in final page', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);
            await cssInteract(page).with('#lastBtn').click();

            //Back and First buttons are not disabled:
            for (const cssValue of ['#backBtn', '#firstBtn']) {
                expect(
                    await cssInteract(page).with(cssValue).get.isDisabled()
                ).toBeFalsy();
            }
            //Next and Last buttons are disabled:
            for (const cssValue of ['#nextBtn', '#lastBtn']) {
                expect(
                    await cssInteract(page).with(cssValue).get.isDisabled()
                ).toBeTruthy();
            }
        });

        it('to go back a page', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);

            await cssInteract(page).with('#lastBtn').click();
            await cssInteract(page).with('#backBtn').click();
            await cssInteract(page).with(nftObject(1)).click();
            const value = await cssInteract(page)
                .with('#objectID')
                .get.textContent();
            expect(value.trim()).toBe('player5');
        });

        it('to go to first page', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);
            await cssInteract(page).with('#lastBtn').click();
            await cssInteract(page).with('#backBtn').click();
            await cssInteract(page).with('#firstBtn').click();
            await cssInteract(page).with(nftObject(1)).click();
            const value = await cssInteract(page)
                .with('#objectID')
                .get.textContent();
            expect(value.trim()).toBe('ChildObjectWBrokenImage');
        });

        it('where first and back disappear in first page', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);
            //Back and First buttons are disabled:
            for (const cssValue of ['#backBtn', '#firstBtn']) {
                expect(
                    await cssInteract(page).with(cssValue).get.isDisabled()
                ).toBeTruthy();
            }
            //Next and Last buttons are not disabled:
            for (const cssValue of ['#nextBtn', '#lastBtn']) {
                expect(
                    await cssInteract(page).with(cssValue).get.isDisabled()
                ).toBeFalsy();
            }
        });
    });
    describe('Group View', () => {
        it('evaluates balance', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);

            expect(
                await cssInteract(page)
                    .with(coinGroup(1).field(1))
                    .get.textContent()
            ).toBe('0x2::USD::USD');

            expect(
                await cssInteract(page)
                    .with(coinGroup(1).field(2))
                    .get.textContent()
            ).toBe('9007199254740993');

            expect(
                await cssInteract(page)
                    .with(coinGroup(2).field(1))
                    .get.textContent()
            ).toBe('SUI');

            expect(
                await cssInteract(page)
                    .with(coinGroup(2).field(2))
                    .get.textContent()
            ).toBe('200');
        });
    });
    describe('Transactions for ID', () => {
        const txResults =
            'TxIdTimeTxTypeStatusAddressesXHTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressYHTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZHTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZITP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZJTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZKTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZLTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZMTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZNTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dressZOTP9gcFmF5K...KFhdqfpdK8=<1secagoTransfer✔From:senderAddressTo:receiv...dress';

        it('are displayed from and to address', async () => {
            const address = 'ownsAllAddress';
            await page.goto(`${BASE_URL}/addresses/${address}`);
            const fromResults = await cssInteract(page)
                .with('#tx')
                .get.textContent();
            expect(fromResults.replace(/\s/g, '')).toBe(txResults);
        });
        it('are displayed for input and mutated object', async () => {
            const address = 'CollectionObject';
            await page.goto(`${BASE_URL}/addresses/${address}`);
            const fromResults = await cssInteract(page)
                .with('#tx')
                .get.textContent();
            expect(fromResults.replace(/\s/g, '')).toBe(txResults);
        });
    });
});
