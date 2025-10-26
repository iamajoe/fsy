# Fsy

> A p2p file syncing tool. Set your push pulls and let the system sync it all between nodes.

***

### Dependencies

1. [rust](https://rust-lang.org/tools/install/)

### Run

1. `cargo run`

### Configuration

After you run the first time, a config will be created under `$HOME/.config/fsy/config.toml`.

#### Note about node_id
`node_id` is the identifier of the environment you are running and it is unique per config. When you run, the `node_id` will be presented and you can use it on the configs of other environments as per the documentation

#### Explanation

```toml
# trustees is the list of nodes you want to interact with
[[nodes]]
# friendly name id of the environment
# node name needs to be unique
name = "desktop"
id = "<env node id>"

[[target_groups]]
# friendly name for the sync to be done, needs to be common to the configs
# target group name needs to be unique
name = "amazing_file"
path = "/Users/joe/amazing_file.txt" # file to sync

# targets is where and how this sync should be done
[[target_groups.targets]]
# there are 3 modes push / pull / pushpull
# - push: only pushes the changes to envs
# - pull: only pulls changes from envs
# - pushpull: bilateral communication of changes
mode = "push"
node_name = "desktop" # trustee friendly name id

[local]
# set of keys to build up your local node id
public_key = "..."
secret_key = [1]
push_debounce_millisecs = 500 # run a push check every x ms
loop_debounce_millisecs = 250 # runs queue and events checks every x ms
```

### TODO
- [ ] Lock mechanism
    1. [ ] Create a backup file upon start of downloading
    2. [ ] On watch file changes, check if there is a bkp file
    3. [ ] Delete it after download
- [ ] Test directories and single files
- [ ] On initialization, perform check for sync
