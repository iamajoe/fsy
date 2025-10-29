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
# friendly name id on the current node environment to be used
# on the target groups target node_name
# node name needs to be unique
name = "desktop"
id = "<env node_id>"

[[target_groups]]
# friendly name for the sync to be done, needs to be common to the 
# node configurations to identify the push/pull
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
secret_key = []
push_debounce_millisecs = 500 # run a push check every x ms
loop_debounce_millisecs = 250 # runs queue and events checks every x ms
```

### TODO
- [ ] Lock mechanism
    1. [x] On file changed event listen, check if there is a lock file. Ignore if there is
    2. [x] Create an empty swp file and lock file upon start of downloading
    3. [x] Download to the swp file
    4. [ ] On error, delete swp file
    5. [x] Upon download done
        1. [x] Remove original file
        2. [x] Move swp to the original path
        3. [x] Remove lock file after x amount of time
- [ ] Test directories and single files
- [ ] On network listen, check for possible new syncs
    - [ ] On start
    - [ ] After network is closed
