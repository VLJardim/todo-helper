# Set up todo-helper

This guide installs the app, connects its four sources (Claude Code sessions, Outlook, itslearning, and Discord), and sets up the shared calendar. Each section takes 2 to 10 minutes and ends with a check. Do the sections in any order; a source you skip just stays quiet.

Secrets (the Discord bot token, the itslearning feed link, the calendar sign-in) go into the app's **Settings** view, which stores them in Windows Credential Manager, or in Keychain on a Mac. Never paste them into a file in this folder.

A status line in **Settings** has a dot: blue means the last check worked, red means it needs you, and gray means it hasn't run yet.

The app runs on Windows and macOS. Outlook (section 4) only works on Windows; everything else works on both.

## 1. Build and install the app

About 10 minutes, most of it waiting for the build.

### On Windows

You need Node.js 22 or later, Rust with the MSVC toolchain, and Visual Studio 2022 Build Tools with the "Desktop development with C++" workload.

1. In PowerShell, go to the project folder: `cd $HOME\Desktop\todo-helper`
2. Install the JavaScript packages: `npm ci`
3. Build the installer: `npm run tauri build`
4. Run the installer: `.\src-tauri\target\release\bundle\nsis\todo-helper_0.1.0_x64-setup.exe`
5. Open **todo-helper** from the Start menu.

### On macOS

You need Node.js 22 or later, Rust (from rustup.rs), and the Xcode Command Line Tools (`xcode-select --install`).

1. In Terminal, go to the project folder, for example `cd ~/Desktop/todo-helper`
2. Install the JavaScript packages: `npm ci`
3. Build the app: `npm run tauri build`
4. Open `src-tauri/target/release/bundle/dmg/todo-helper_0.1.0_aarch64.dmg` and drag **todo-helper** to Applications.
5. Open **todo-helper** from Applications. If macOS says it can't check the app, right-click it and choose **Open**.

**Check:** the board opens with Inbox, four weeks of days, Doing, and Done. Type `Test card` in the Inbox field and press Enter; the card appears at the top of Inbox.

## 2. Claude Code (needed for email and Discord cards)

About 2 minutes. The app asks Claude Code to turn emails and Discord messages into cards, using your Claude subscription. Install Claude Code first if you don't have it.

1. In PowerShell (Terminal on a Mac), run `claude`.
2. If it asks you to log in, follow its steps.
3. Type `/exit` and press Enter.

**Check:** `claude -p "Reply with OK"` prints `OK`.

## 3. Claude Code sessions in the Sessions panel

About 3 minutes. A hook is a command Claude Code runs on events such as a session start. These hooks append each event to `.claude/todo-helper/events.jsonl` in your home folder, and the app reads that file. This changes your global Claude Code settings, for every project.

1. In PowerShell (Terminal on a Mac), go to the project folder: `cd $HOME\Desktop\todo-helper`
2. Run `node hooks/install.mjs`. It adds 8 hook entries to `.claude/settings.json` in your home folder, keeps your existing hooks and settings, and first saves your current file as `settings.json.before-todo-helper`.

**Check:** start a new Claude Code session in any folder, then open **Sessions** in the app. The session appears within 5 seconds. Ask the session to launch an agent; "1 agent running" appears with the agent's description.

To undo it, run `node hooks/install.mjs --uninstall`. It removes only these hooks and keeps every other setting. If you move this project folder, run step 2 again from the new folder; it replaces the old entries.

## 4. Outlook (school, work, and personal mail)

About 2 minutes. The app reads new Inbox mail from every account in classic Outlook for Windows, the Outlook with a **File** menu. It never marks mail as read or changes anything, and it needs no password: Outlook is already signed in. The new Outlook doesn't work for this.

1. Open classic Outlook and add every account you want on the board.
2. Make classic Outlook start with Windows: press Windows + R, type `shell:startup`, press Enter, and drag a shortcut to Outlook into the folder that opens.
3. In the app, open **Settings** and select **Check now** in the Outlook section.

**Check:** the Outlook dot turns blue and the line says "Connected", with the number of accounts. Then send an email that says "Hand in the test essay by Friday" to one account. Within 5 minutes (or after Check now), a card due on Friday appears in Inbox.

The first check reads only the last 3 days of mail. If Outlook is closed, the line says so; the app catches up at the next check after you open it.

## 5. itslearning

About 3 minutes. The app reads your itslearning calendar through its private subscription link, every 30 minutes. It never sees your school password.

1. Sign in to itslearning in a browser.
2. Open **Calendar**.
3. Select **Subscribe**. In Danish it may say **Abonnér**; it may also sit in the calendar's menu or be called **Export** or **iCal**.
4. Copy the link it shows. It starts with `https://` or `webcal://`.
5. In the app, open **Settings**.
6. Paste the link into **Calendar feed link**.
7. Select **Save** next to it.
8. Select **Check now** in the itslearning section.

**Check:** the itslearning dot turns blue, and the line says how many events are in the next 26 weeks. Your lessons show in the Calendar view at their hours; the board shows only hand-in deadlines.

Treat the link like a password: anyone with it can read your calendar. Repeating events count only their first date.

## 6. Discord

About 10 minutes. The app reads the channels you choose through a bot, every 5 minutes. Adding a bot to a server needs the **Manage Server** permission there; if you don't have it, send the URL from step 16 to the server's admin and ask them to do steps 17 and 18.

1. Open https://discord.com/developers/applications and sign in.
2. Select **New Application**.
3. Type `todo-helper` as the name.
4. Accept the terms, then select **Create**.
5. In the left menu, select **Bot**.
6. Under **Privileged Gateway Intents**, turn on **Message Content Intent**.
7. Select **Save Changes**.
8. Select **Reset Token**, then confirm.
9. Copy the token.
10. In the app, open **Settings**.
11. Paste the token into **Bot token**.
12. Select **Save** next to it.
13. Back in the Developer Portal, select **OAuth2** in the left menu.
14. In **OAuth2 URL Generator**, tick **bot**.
15. Under **Bot Permissions**, tick **View Channels** and **Read Message History**.
16. Copy the generated URL at the bottom.
17. Open the URL in a browser.
18. Pick the server, then select **Authorize**.
19. In the Discord app, open **User Settings**, then **Advanced**.
20. Turn on **Developer Mode**.
21. Right-click a channel to watch, then select **Copy Channel ID**.
22. In the app's **Settings**, paste the ID into **Channel IDs**. For more channels, repeat step 21 and add each ID after a comma.
23. Press Tab to save the IDs.
24. Select **Check now** in the Discord section.

**Check:** the Discord dot turns blue and the line starts with "Connected." Then post "everyone bring your laptop Friday" in a watched channel. Within 5 minutes (or after Check now), a card about bringing a laptop appears in Inbox. A chat message like "haha nice" makes no card.

The first check reads only the last 2 days of each channel. Like email, each new message's text goes to Claude once to find todos, and is then deleted from the app's database.

## 7. Shared calendar: create it (the calendar owner, once)

About 10 minutes. One person, the calendar owner, sets this up in their own Supabase project (the free plan is enough). The calendar lives in its own schema, `todo_helper`, apart from the project's other tables. Every member can add, change, and delete any event, and the project logs who did what. Anyone who can sign in to the project is a member.

1. Sign in at https://supabase.com/dashboard and create or open your project.
2. Open **SQL Editor**, paste all of `supabase/schema.sql`, and select **Run**.
3. Open **Integrations**, then **Data API**, then the **Settings** tab. Under **Exposed schemas**, tick `todo_helper`, and select **Save**.
4. Paste all of `supabase/check.sql` in a new query and select **Run**. It should say "All checks passed".
5. Open **Authentication**, then **Sign In / Providers**, and check that **Allow new users to sign up** is off.
6. Open **Authentication**, then **Users**, and add each member, yourself included. To add one, select **Add user**, then **Create new user**, type their email and a starting password, keep **Auto Confirm User** ticked, and select **Create user**.
7. Open **Project Settings**, then **API Keys**, and copy the **Publishable key** (it starts with `sb_publishable_`, or with `eyJ` for the legacy anon key). Copy the **Project URL** from **Connect** at the top of the dashboard (it looks like `https://abcdefgh.supabase.co`).
8. Send each member the Project URL, the publishable key, and their starting password. Send the password on its own, not in the same message.

**Check:** step 4 says "All checks passed", and the sign-up switch from step 5 is off. Never share the **secret** key: it skips every rule.

To put back a deleted event, open **Table Editor**, then `event_log`, find the row that says `deleted`, and copy its `before` values into a new row in `events`.

## 8. Shared calendar: sign in (each member)

About 2 minutes. You need the Project URL, the publishable key, and your starting password from the calendar owner (section 7, step 8).

1. In the app, open **Settings** and find **Shared calendar**.
2. Paste the Project URL into **Project URL** and the key into **Publishable key**. Each saves when you leave the field.
3. Type your email and starting password, and select **Sign in**.
4. Type your name the way the others should see it, and select **Save**.
5. Select **Change password**, type a new password of at least 8 characters, and select **Save**.

**Check:** Settings says "Signed in as" with your name, and the calendar's dot turns blue. Open **Calendar** and add an event; it appears on another member's computer within a minute.

You stay signed in after a restart: the sign-in is kept in Windows Credential Manager or Keychain, never in the app's database. **Sign out** removes it and the calendar's copy on this computer.

## When a status line needs you

Each source's status line in **Settings** says what went wrong and what to do. The common ones:

| The status line says | Do this |
|---|---|
| Not set up | Fill in the fields the line names, then select Check now. |
| Outlook isn't running | Open classic Outlook (section 4, step 2 makes it start with Windows), then select Check now. |
| Discord refused the bot token | Reset the token and save it again (section 6, steps 8 to 12). |
| The bot can't read channel ... | In Discord, give the bot's role **View Channel** and **Read Message History** in that channel. |
| Channel ... doesn't exist, or the bot isn't in its server | Check the channel ID, then repeat section 6, steps 17 and 18. |
| Messages came back without text | Turn on **Message Content Intent** (section 6, steps 6 and 7). This line has a blue dot, because the check itself worked. |
| itslearning refused the feed link | Copy a new link (section 5) and save it. |
| Claude Code isn't installed, or `claude` isn't on the PATH | Install Claude Code, then restart the app. |
| Claude Code failed ... log in | Do section 2. |
