# General
language-set = Language set successfully.
username-prompt = Hello! Please enter a username for registration.
username-taken = Sorry, this username is already taken. Please choose another username.
username-check-error = An error occurred while checking the username. Please try again later.
username-empty-error = Username cannot be empty. Please enter a valid username.
password-prompt = Now enter a password.
password-empty-error = Password cannot be empty. Please enter a valid password.
nickname-prompt-choice = Your username will be '{ $username }'. Would you like to set a different nickname? If not, your nickname will be the same as your username.
nickname-prompt-enter = Please enter your desired nickname.
nickname-empty-error = Nickname cannot be empty. Please enter a valid nickname.
register-success = User { $username } successfully registered.
register-success-db-sync-issue = Your TeamTalk account is ready, but there was an issue syncing your registration locally. Please contact an administrator if you experience issues.
register-error = Registration error. Please try again later or contact an administrator.
already-registered = You have already registered one TeamTalk account from this Telegram account. Only one registration is allowed.
admin-approval-sent = Registration request sent to administrators. Please wait for approval.
admin-approved = Your registration has been approved by the administrator. You can now use TeamTalk.
admin-rejected = Your registration has been declined by the administrator.
deeplink-disabled = Deeplink registration is currently disabled in the configuration.
deeplink-invalid = This registration link is invalid, expired, or has already been used.
deeplink-used-already = You have already registered. This link cannot be used to register again.
deeplink-bot-username-missing = Internal error: bot username is not available. Please contact support.
bot-shutdown = Shutting down...

# New messages (Host/Port/Broadcast)
msg-host = Host: { $host }
msg-port = Port: { $port }
tt-broadcast-registration = User { $username } was registered.

# Buttons
btn-yes = Yes
btn-no = No (use username)
btn-admin-verify = Yes
btn-admin-reject = No
btn-delete-user = Delete User
btn-manage-banlist = Manage Ban List
btn-list-tt-accounts = List TeamTalk Accounts
btn-unban = Unban
btn-add-ban-manual = Add to Ban List Manually
btn-confirm-delete = Confirm Delete
btn-cancel = Cancel
btn-delete-from-tt = Delete from TeamTalk

tt-account-admin = TeamTalk Admin
tt-account-user = TeamTalk User
tt-account-type-prompt = This TeamTalk account will be for username '{ $username }'. Do you want to register it as a TeamTalk 'Admin' or a regular 'User' on the server?

# Admin Messages
admin-request-title = Registration request:
admin-request-username = Username:
admin-request-nickname = Nickname:
admin-request-telegram-user = Telegram User:
admin-request-approve = Approve registration?
admin-submit-error = An error occurred while submitting your registration for approval. Please try again later or contact an administrator.
username-not-found = Error: Username not found. Please start over.
invalid-choice = Invalid choice. Please try again.
admin-panel-title = Admin Panel
admin-no-users = No registered users found to delete.
admin-select-delete = Select a user to delete:
admin-user-deleted = User with Telegram ID { $tg_id } has been deleted and banned.
admin-banlist-empty = The ban list is empty.
admin-banlist-title = Banned Users:
admin-unbanned = User { $tg_id } has been unbanned.
admin-unban-no-target = Error: No target user ID specified for unban.
admin-unban-fail = Failed to unban user { $tg_id }.
admin-action-refresh-fail = Action processed. Could not refresh list immediately.
admin-ban-prompt = Please enter the Telegram ID and reason for the ban on separate lines.
admin-ban-success = User { $tg_id } has been manually banned.
admin-ban-invalid = Invalid Telegram ID provided.
admin-ban-fail = Failed to manually ban user { $tg_id }.
admin-tt-list-error = Could not connect to the TeamTalk server to get the list of accounts.
admin-tt-no-accounts = No TeamTalk accounts found on the server.
admin-tt-list-title = TeamTalk Accounts:
admin-tt-delete-prompt = Are you sure you want to delete the TeamTalk user '{ $tt_username }'?
admin-tt-deleted = TeamTalk user '{ $tt_username }' was successfully deleted.
admin-tt-delete-fail = Failed to delete TeamTalk user '{ $tt_username }'. Reason: { $error }
admin-req-approved-alert = User { $username } registration approved.
admin-req-rejected-alert = User { $username } registration declined.
admin-req-not-found = Registration request not found, outdated, or already processed.
admin-req-handled = This registration request has already been handled.
admin-approve-failed-critical = CRITICAL: Registration for { $username } was approved, but the final registration step failed. Please check logs.
deeplink-generate-error = An error occurred while generating the deeplink.
admin-decision-notify = Admin { $admin_name } ({ $admin_id }) has { $decision } the registration request for TeamTalk user '{ $teamtalk_username }' (Telegram ID: { $registrant_telegram_id }).
admin-decision-telegram-username =  Telegram Username: @{ $registrant_tg_username }
admin-decision-approved = approved
admin-decision-rejected = rejected

# Files
file-caption = Your .tt file for quick connection
link-text = Or use this TT link:\n
file-send-error = Could not send the .tt file or link. Please contact an admin.

# Commands

# TT Worker Notifications
tt-account-removed = TeamTalk: User account '{ $username }' has been REMOVED.
tt-account-removed-banned = 🚫 User '{ $username }' removed from TT. Auto-banned TG ID: { $tg_id }
tt-account-removed-no-link = 🗑️ User '{ $username }' removed from TT (No TG link found).

# Web Interface
web-title = TeamTalk Registration
web-header = TeamTalk Registration
web-intro-line-1 = If you want to register on the server
web-intro-line-2 = please fill out the form below.
web-select-language = Select Language:
web-language-label = Language:
web-set-language = Set Language
web-success-title = Registration successful!
web-download-msg = You can now download your configuration:
web-link-tt = Download .tt file
web-link-zip = Download pre-configured TeamTalk Client (ZIP)
web-quick-link = Quick Connect Link:
web-countdown-text = You have <span id='countdown-timer'>10:00</span> to download your .tt file, client or use the quick connect link.
web-expired = expired
web-second = second
web-seconds-few = seconds_few
web-seconds = seconds
web-label-username = Username:
web-label-nickname = Nickname (optional):
web-placeholder-nickname = Defaults to username if blank
web-label-password = Password:
web-show-password = Show Password
web-btn-register = Register
web-err-ip-limit = This IP address has already been used to register an account.
web-err-username-taken = Sorry, this username is already taken. Please choose another one.
web-err-timeout = Timeout waiting for TeamTalk server.
web-err-file-not-found = File not found on disk
web-err-invalid-link = Invalid or expired link

