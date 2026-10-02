# project description

a rust app for checking your work in azure devops. uses ratatui  https://crates.io/crates/ratatui as tui library

## configuration
save config in $XDG_CONFIG_HOME. a json file
some of the options should be 
- project
- colors  (everything we set color on. as default we should use catppuccin moca colors)

## auth
find the best way to auth to azure devops.

oath, cli login or access token. Save results securly.

## views

### start

two columns
#### left column
left about 25% of width
left colum should have some boxes.
- My PRs
- my workitems
- other peaoples PRs
- other workitems that are in ready colum for this sprint.

#### right column
should be one row the heigt of the window
should swho info for what is selected in left column

- if pr: show PR info with comments and statuses for builds and other things. and if someone hass approved
- if workitem: show work item

you should be able to change statate for workitem. and some of outr board columns have av extra state. Board Column Done set tue true is done
our columns are new. analyze, refine ready, active (doing, done), FUnctional test, closed. but try to get them dynamicly.

### toolbar
where to show action keys and other futrutre information

## navigation
yopu should be able to use h j k l and arrows for navigating. these should be called direction
ctrl + direction changes column and row
enter chooses iten in left column to display in right column

## actions
actions are things you can do with prs and stories. we should use keys for actions. actions should be displayed in 

### Pr
- open in browser

### workitems
- change statuses
- assign to me
- open in browser
