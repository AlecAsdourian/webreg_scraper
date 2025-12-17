import * as puppeteer from "puppeteer";
import {Context, WebRegLoginResult} from "./types";

export const PUSH = "push";

export const NUM_ATTEMPTS_BEFORE_EXIT: number = 6;
const WEBREG_URL: string = "https://act.ucsd.edu/webreg2/start";

/**
 * Prints a help message explaining how the program works.
 */
export function printHelpMessage(): void {
    console.log("webreg automatic login script: automatically get updated session cookies for webreg.");
    console.log("usage 1: for any term.")
    console.log("\tusage: node index.js --port <port> [--debug]");
    console.log("\t\twhere <port> is an integer.");
    console.log("\t\twhere --debug is used if you want to enable debug mode");
    console.log("\texample: node index.js 3000");

    console.log("usage 2: for a specific term.");
    console.log("\tusage: node index.js --port <port> [--term <term>] [--debug]");
    console.log(`\t\twhere <term> starts with a term (e.g., SP, WI) and ends with the year (e.g., 24, 21).`);
    console.log("\t\twhere <port> is an integer.");
    console.log("\t\twhere --debug is used if you want to enable debug mode");
    console.log("\texample: node index.js SP22 3000");
}

export const TERM_ARR: { [term: string]: [number, number] } = {
    "SP": [5200, 22], // SP22
    "S1": [5210, 22], // S122
    "S2": [5220, 22], // S222
    "S3": [5230, 22], // S322
    "SU": [5240, 22], // SU22
    "FA": [5250, 22], // FA22
    "WI": [5260, 23], // WI23
};

/**
 * Gets the sequence ID associated with the specified term.
 * @param termYear The term.
 * @return The sequence ID, or `0` if the term passed in is invalid.
 */
export function getTermSeqId(termYear: string): number {
    if (termYear.length !== 4) {
        return 0;
    }

    const term = termYear.substring(0, 2);
    if (!(term in TERM_ARR)) {
        return 0;
    }

    const [baseSeqId, baseYear] = TERM_ARR[term];

    const year = Number.parseInt(termYear.substring(2), 10);
    if (Number.isNaN(year)) {
        return 0;
    }

    return 70 * (year - baseYear) + baseSeqId;
}

/**
 * Logs a message.
 * @param term The term to display this log with.
 * @param msg The message to log.
 */
export function logNice(term: string, msg: string): void {
    const time = new Intl.DateTimeFormat([], {
        timeZone: "America/Los_Angeles",
        year: "numeric",
        month: "numeric",
        day: "numeric",
        hour: "numeric",
        minute: "numeric",
        second: "numeric",
    }).format(new Date());
    console.info(`[${time}] [${term}] ${msg}`);
}

/**
 * Waits a certain number of milliseconds before continuing execution.
 * @param ms The number of milliseconds to wait.
 */
export function waitFor(ms: number): Promise<void> {
    return new Promise(async r => {
        setTimeout(() => {
            r();
        }, ms);
    });
}

/**
 * Gets new WebReg session cookies. This assumes that
 * - your WebReg credentials are correct, and
 * - Duo Push is automatically activated upon reaching the Duo 2FA page.
 *
 * Note that calling this function does take some time to finish, upwards of 30
 * seconds in some cases.
 *
 * @returns One of
 * - your cookie string, if available.
 * - an empty string, if an issue occurred when attempting to either authenticate
 * with Duo 2FA (e.g., could not load the 2FA page) or when trying to access WebReg
 * in general.
 * - `"ERROR UNABLE TO AUTHENTICATE."`, if the script is unable to log into WebReg
 * after a certain number of tries.
 */
export async function fetchCookies(ctx: Context, browser: puppeteer.Browser, isInit: boolean): Promise<string> {
    const termLog = ctx.termInfo?.termName ?? "ALL";
    logNice(termLog, "GetCookies function called.");

    let numFailedAttempts = 0;
    while (true) {
        // Close any unnecessary pages.
        let pages = await browser.pages();
        while (pages.length > 1) {
            await pages.at(-1)!.close();
            pages = await browser.pages();
        }

        const page = await browser.newPage();
        try {
            logNice(termLog, "Opened new page. Attempting to connect to WebReg site.")
            const resp = await page.goto(WEBREG_URL);
            // If we somehow cannot reach the page, try again.
            if (!resp) {
                numFailedAttempts++;
                logNice(termLog, `Unable to open page. Retrying (${numFailedAttempts}/${NUM_ATTEMPTS_BEFORE_EXIT}).`);
                continue;
            }

            logNice(termLog, `Reached ${resp.url()} with status code ${resp.status()}.`);
            if (resp.status() < 200 || resp.status() >= 300) {
                throw new Error("Non-OK Status Code Returned.");
            }
        } catch (e) {
            // Timed out probably, or failed to get page for some reason.
            logNice(termLog, "An error occurred when trying to reach WebReg. See error stack trace below.");
            console.info(e);
            console.info();
            return "";
        }

        await waitFor(3000);
        const content = await page.content();
        // This assumes that the credentials are valid.
        if (content.includes("Signing on using:") && content.includes("TritonLink user name")) {
            logNice(termLog, "Attempting to sign in to TritonLink.");
            // https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_Selectors
            await page.type('#ssousername', ctx.webreg.username);
            await page.type('#ssopassword', ctx.webreg.password);
            await page.click('button[type="submit"]');
        }

        // Wait for either Duo 2FA prompt (if we need 2FA) or "Go" button (if no 2FA needed) to show up
        logNice(termLog, "Waiting for Duo 2FA prompt or 'Go' button to show up.");

        let loggedIn = false;
        const r: WebRegLoginResult = await Promise.race([
            // Either wait for the 'Go' button to show up, which implies that we
            // have an authenticated session, **OR** wait for the Duo prompt
            // to show up.
            //
            // If an error occurred, it means the 'Go' button could not be found
            // after 30 seconds. This implies that the Duo prompt could not be
            // found since *if* the Duo prompt did show up, then the error would
            // have never occurred.

            // Here, we wait for the 'Go' button (to load WebReg for a term) to
            // show up.
            (async () => {
                try {
                    await page.waitForSelector("#startpage-button-go", {visible: true, timeout: 30 * 1000});
                } catch (_) {
                    // conveniently ignore the error
                    return WebRegLoginResult.UNKNOWN_ERROR;
                }

                loggedIn = true;
                return WebRegLoginResult.LOGGED_IN;
            })(),
            // Here, we *repeatedly* check to see if the Duo 2FA prompt is visible AND some components of
            // the prompt (in our case, the "Other Option" button) are visible.
            (async () => {
                const interval = await new Promise<NodeJS.Timeout>(r => {
                    const internalInterval = setInterval(async () => {
                        try {
                            // If we're logged in, then we can stop the interval.
                            if (loggedIn) {
                                r(internalInterval);
                                return;
                            }

                            // Check if the header text is visible (Check for a Duo Push)
                            const duoPrompt = await page.$("#header-text");
                            if (!duoPrompt) {
                                return;
                            }

                            // "Other Options" selector - is it visible?
                            if (!(await page.$("#auth-view-wrapper > div:nth-child(2) > div.row.display-flex.other-options-link.align-flex-justify-content-center.size-margin-bottom-large.size-margin-top-small > a"))) {
                                return;
                            }

                            r(internalInterval);
                        } catch (e) {
                            // Conveniently ignore the error
                        }
                    }, 500);
                });

                clearInterval(interval);
                return WebRegLoginResult.NEEDS_DUO;
            })()
        ]);

        // If we hit this, then we just try again.
        if (r === WebRegLoginResult.UNKNOWN_ERROR) {
            // If too many failed attempts, then notify the caller.
            // After all, we don't want to make too many Duo pushes and get
            // the AD account blocked by ITS :)
            if (numFailedAttempts >= NUM_ATTEMPTS_BEFORE_EXIT) {
                logNice(termLog, "Unable to authenticate due to too many attempts reached, giving up.")
                return "ERROR UNABLE TO AUTHENTICATE.";
            }

            numFailedAttempts++;
            logNice(termLog, `Unable to find a 'Go' button or Duo 2FA frame. Retrying (${numFailedAttempts}/${NUM_ATTEMPTS_BEFORE_EXIT}).`);
            continue;
        }

        // No go button means we need to log in.
        if (r === WebRegLoginResult.NEEDS_DUO) {
            logNice(termLog, "Duo 2FA prompt required.");

            // If we have already initialized this and we are asked to do a Duo 2FA prompt, then
            // cancel.
            if (!isInit && ctx.loginType === PUSH) {
                logNice(termLog, "Attempting to send request to Duo, but this wasn't supposed to happen");
                throw new Error("ruby is bad");
            }

            logNice(termLog, "Accept the Duo 2FA prompt when you receive it. You may need to open the app.");
            // Need to find the Duo prompt ("Check for a Duo Push") so we can actually authenticate
            const duoPrompt = await page.$("#header-text");
            if (!duoPrompt) {
                logNice(termLog, "No possible Duo prompt found. Returning empty string.");
                console.info();
                throw new Error();
            }

            logNice(termLog, "Waiting for the user to accept the Duo 2FA prompt.");
            try {
                // We're now going to wait until the trust prompt shows up, which should happen
                // once the user responds to the Duo push request.
                await page.waitForSelector("#trust-browser-button", {
                    timeout: 42000
                });
            } 
            catch (e) {
                logNice(termLog, "Cannot find the 'Is this your device?' prompt. Did you accept the Duo request?");
                console.info(e);
                console.info();
                return "";
            }
    
            logNice(termLog, "Duo 2FA prompt responded to. Now telling Duo to trust this browser.");
            await waitFor(1000);
            // Once the button shows up, then press that button -- this is equivalent to the "Remember me
            // for 7 days."
            await page.click("#trust-browser-button");
            logNice(termLog, "Clicked on 'Yes, this is my device' prompt.");
        }
        else {
            logNice(termLog, "'Go' button found. No 2FA needed.");
        }

        // Now, we can just wait until the 'Go' button shows up.
        try {
            await Promise.all([
                page.waitForSelector("#startpage-select-term", {visible: true}),
                page.waitForSelector('#startpage-button-go', {visible: true})
            ]);
        } catch (e) {
            // If this hits, then somehow the Go button (for loading WebReg with that term)
            // didn't load at all. This is rare, although it does happen from time to time
            // for reasons I have yet to understand.
            //
            // Note that I used a try/catch in Promise.all instead of Promise.allSettled
            // because waitForSelector apparently throws the error instead of rejecting?
            // Not sure if there's a way to handle that without try/catch
            logNice(termLog, "Cannot find the 'Is this your device?' prompt.");
            console.info(e);
            console.info();
            return "";
        }

        logNice(termLog, "Logged into WebReg successfully.");

        let urlToFetch: string = "https://act.ucsd.edu/webreg2/svc/wradapter/get-term";
        if (ctx.termInfo) {
            const termName = ctx.termInfo.termName;
            const termSelector = `${ctx.termInfo.seqId}:::${termName}`;
            await page.select("#startpage-select-term", termSelector);
            // Get cookies ready to load.
            await page.click('#startpage-button-go');
            urlToFetch = `https://act.ucsd.edu/webreg2/svc/wradapter/secure/sched-get-schednames?termcode=${termName}`;
        }

        const cookies = await page.cookies(urlToFetch);
        logNice(termLog, `Extracted cookies for term '${termLog}' and responding back with them.\n`);

        if (ctx.session.start === 0) {
            ctx.session.start = Date.now();
        } else {
            ctx.session.callHistory.push(Date.now());
        }

        return cookies.map(x => `${x.name}=${x.value}`).join("; ");
    }
}

/**
 * Scrapes degree audit data by navigating through the degree audit system.
 * This function:
 * 1. Ensures user is logged in (reuses existing session from fetchCookies)
 * 2. Navigates to degree audit list page
 * 3. Triggers audit creation if needed
 * 4. Extracts the most recent audit ID
 * 5. Navigates to audit read page
 * 6. Scrapes and returns the audit data as JSON
 *
 * @param ctx The context containing credentials and session info
 * @param browser The Puppeteer browser instance
 * @returns JSON object containing degree audit data
 */
export async function fetchDegreeAudit(ctx: Context, browser: puppeteer.Browser): Promise<any> {
    const termLog = ctx.termInfo?.termName ?? "N/A";
    logNice(termLog, "Starting degree audit scrape");

    const page = await browser.newPage();

    try {
        // Step 1: Navigate to degree audit list page
        logNice(termLog, "Navigating to degree audit list page");
        await page.goto("https://act.ucsd.edu/studentDarsSelfservice/audit/list.html", {
            waitUntil: 'networkidle2',
            timeout: 30000
        });

        // Check if we need to log in (if session expired)
        const content = await page.content();
        if (content.includes("TritonLink user name") || content.includes("Sign In")) {
            logNice(termLog, "Session expired, re-authenticating for degree audit");
            // Reuse the fetchCookies authentication flow
            await fetchCookies(ctx, browser, false);
            // Navigate again after login
            await page.goto("https://act.ucsd.edu/studentDarsSelfservice/audit/list.html", {
                waitUntil: 'networkidle2'
            });
        }

        // Step 2: Check if we need to create a new audit
        // Look for existing audits on the list page
        const pageContent = await page.content();

        // Try to find existing audit links
        // The audit list has links like: read.html?id=JobQueueRun!!!!...
        let auditIdMatch = pageContent.match(/read\.html\?id=([^"]+)/);

        if (!auditIdMatch) {
            // No existing audits found, need to create one
            logNice(termLog, "No existing audit found, attempting to create new audit");

            try {
                // Navigate to create page
                await page.goto("https://act.ucsd.edu/studentDarsSelfservice/audit/create.html", {
                    waitUntil: 'networkidle2',
                    timeout: 15000
                });

                // Look for form submit button or link
                // Try multiple possible selectors
                const submitButton = await Promise.race([
                    page.waitForSelector('input[type="submit"]', { timeout: 5000 }).catch(() => null),
                    page.waitForSelector('button[type="submit"]', { timeout: 5000 }).catch(() => null),
                    page.waitForSelector('input[value*="Run"]', { timeout: 5000 }).catch(() => null),
                ]);

                if (submitButton) {
                    await submitButton.click();
                    // Wait for redirect back to list page
                    await page.waitForNavigation({ waitUntil: 'networkidle2', timeout: 15000 });

                    // Wait a bit for audit to generate
                    logNice(termLog, "Waiting for audit to generate (5 seconds)");
                    await new Promise(resolve => setTimeout(resolve, 5000));

                    // Get the new audit ID
                    const newPageContent = await page.content();
                    auditIdMatch = newPageContent.match(/read\.html\?id=([^"]+)/);

                    if (!auditIdMatch) {
                        throw new Error("Failed to find audit ID after creation");
                    }
                } else {
                    throw new Error("Could not find submit button on create page");
                }
            } catch (error) {
                logNice(termLog, `Failed to create new audit: ${error}`);
                throw error;
            }
        }

        // Step 3: Extract audit ID and navigate to read page
        const finalPageContent = await page.content();
        const finalAuditIdMatch = finalPageContent.match(/read\.html\?id=([^"]+)/);

        if (!finalAuditIdMatch) {
            throw new Error("Unable to find audit ID");
        }

        const auditId = finalAuditIdMatch[1];
        logNice(termLog, `Found audit ID: ${auditId}`);

        // Navigate to the audit read page
        const readUrl = `https://act.ucsd.edu/studentDarsSelfservice/audit/read.html?id=${auditId}`;
        logNice(termLog, `Navigating to: ${readUrl}`);
        await page.goto(readUrl, { waitUntil: 'networkidle2' });

        // Step 4: Extract audit data from the page
        logNice(termLog, "Extracting degree audit data from HTML");

        // This will need to be customized based on actual HTML structure
        // For now, return the full HTML content for manual inspection
        const auditHtml = await page.content();

        // TODO: Parse the HTML to extract structured data
        // For now, we'll return basic info + full HTML
        const auditData = {
            auditId: auditId,
            scrapedAt: new Date().toISOString(),
            url: readUrl,
            html: auditHtml,
            // These will be populated once we parse the HTML:
            // studentInfo: { ... },
            // requirements: [ ... ],
            // completedCourses: [ ... ]
        };

        logNice(termLog, "Degree audit scrape complete\n");
        await page.close();

        return auditData;

    } catch (error) {
        logNice(termLog, `Error scraping degree audit: ${error}`);
        await page.close();
        throw error;
    }
}
