use std::io::Write; // access flush()

use rand::Rng; // access gen()
use solana_sdk::instruction::AccountMeta;
use solana_sdk::signature::Signer; // To access pubkey from keypair.

fn keys_gen() -> solana_sdk::signature::Keypair {
    let seed = rand::thread_rng().gen::<[u8; 32]>();
    solana_sdk::signer::keypair::keypair_from_seed(&seed).unwrap()
}

fn airdrop(
    client: &solana_client::rpc_client::RpcClient,
    dst: &solana_sdk::pubkey::Pubkey,
    amount: u64,
) {
    let balance_init = client.get_balance(dst).unwrap();
    let balance_target = balance_init + amount;

    eprint!("airdrop requesting");
    client.request_airdrop(dst, amount).unwrap();
    eprintln!(".");

    // XXX This confirmation retry loop is a bit suspect, but I wasn't able to
    //     make confirmation work with
    //
    //         confirm_transaction_with_spinner
    //
    //     which executed successfully, but did not actually mean that the
    //     account balance was already updated:
    //
    //     client
    //         .confirm_transaction_with_spinner(
    //             // TODO What's a spinner?
    //             &client.request_airdrop(&dst, amount).unwrap(),
    //             &client.get_latest_blockhash().unwrap(),
    //             solana_sdk::commitment_config::CommitmentConfig::confirmed(),
    //         )
    //         .unwrap();
    //
    eprint!("airdrop confirming ");
    let mut backoff = 1;
    while client.get_balance(dst).unwrap() < balance_target {
        eprint!(".");
        std::thread::sleep(std::time::Duration::from_secs(backoff));
        backoff *= 2;
    }
    eprintln!(".");
    eprintln!("airdrop done");
}

fn echo_loop(
    client: &solana_client::rpc_client::RpcClient,
    program_id: solana_sdk::pubkey::Pubkey,
    payer_keys: &solana_sdk::signer::keypair::Keypair,
) {
    let payer_id = payer_keys.pubkey();
    let mut buf_in = String::new();
    let mut stdout = std::io::stdout();
    let stdin = std::io::stdin();

    loop {
        print!("> ");
        stdout.flush().unwrap();
        stdin.read_line(&mut buf_in).unwrap();
        let buf_in_len = buf_in.as_bytes().len();
        let buf_out_keys = keys_gen();
        let buf_out_id = buf_out_keys.pubkey();
        let ix_allocate_buf_out =
            // An account data length cannot be altered once created, so we
            // create a fresh one to accomodate arbitrary length input from the
            // user.
            solana_program::system_instruction::create_account(
                // Buffer account, which:
                // - program writes to;
                // - client reads from.
                &payer_id,
                &buf_out_id,
                client
                    .get_minimum_balance_for_rent_exemption(buf_in_len)
                    .unwrap(),
                buf_in_len as u64,
                &program_id,
            );
        let ix_echo = solana_sdk::instruction::Instruction::new_with_bytes(
            program_id,
            buf_in.as_bytes(),
            vec![{
                // buf_out doesn't have to sign.
                let is_signer = false;
                AccountMeta::new(buf_out_id, is_signer)
            }],
        );
        let ixs = [ix_allocate_buf_out, ix_echo];
        let tx = solana_sdk::transaction::Transaction::new(
            &[payer_keys, &buf_out_keys],
            solana_sdk::message::Message::new(&ixs, Some(&payer_id)),
            client.get_latest_blockhash().unwrap(),
        );
        let sig = client.send_and_confirm_transaction(&tx).unwrap();
        let buf_out = client.get_account(&buf_out_id).unwrap().data;
        print!("< {}", std::str::from_utf8(&buf_out).unwrap());
        stdout.flush().unwrap();
        eprintln!(": {}", sig);
        buf_in.clear();
    }
}

fn main() {
    let program_keypair_file_path = std::env::args().nth(1).unwrap();
    let cluster_url = std::env::args().nth(2).unwrap();

    // XXX We do not strictly need the program's keypair, but only its pubkey.
    //     However, during experimentation, the program's keys will likely
    //     change many times and so it is easier to simply lookup the pubkey
    //     from the keypair found in the default location:
    //     target/deploy/program-keypair.json
    let program_id = solana_sdk::signer::keypair::read_keypair_file(
        program_keypair_file_path,
    )
    .unwrap()
    .pubkey();
    let client = solana_client::rpc_client::RpcClient::new(cluster_url);

    // Using a wallet keypair as payer, I get this error:
    //
    // "Transaction simulation failed: This account may not be used to pay transaction fees"
    //
    // Generating a fresh payer and airdroping seems the simplest for now.
    let payer_keys = keys_gen();
    {
        // XXX airdrop caps:
        //     - devnet  : 2 sol
        //     - testnet : 1 sol
        let sol = 1;
        let lamports = sol * 1_000_000_000;
        airdrop(&client, &payer_keys.pubkey(), lamports);
    }

    echo_loop(&client, program_id, &payer_keys);
}
