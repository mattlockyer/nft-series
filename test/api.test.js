const fs = require('fs');
const assert = require('assert');
const testUtils = require('./test-utils');
const nearAPI = require('near-api-js');
const BN = require('bn.js');
const {
	utils: { format: { parseNearAmount, formatNearAmount } },
	transactions: { deployContract, functionCall }
} = nearAPI;

const {
	gas,
	contractId,
	contractAccount,
	getAccount,
	createOrInitAccount,
	getAccountBalance,
} = testUtils;

const TOKEN_DELIMETER = ':';
const CONTRACT_TOKEN_DELIMETER = '||';

describe('NFT Series', function () {
	this.timeout(30000);

	const now = Date.now().toString();
	let token_type_title = 'dog-' + now;
	let token_id;

	/// users
	const aliceId = 'alice-' + now + '.' + contractId;
	const bobId = 'bob-' + now + '.' + contractId;
	const marketId = 'market.' + contractId;
	let alice, bob, market;
	it('should create user & contract accounts', async function () {
		alice = await getAccount(aliceId);
		bob = await getAccount(bobId);
		console.log('\n\n created:', aliceId, '\n\n');

		market = await createOrInitAccount(marketId);
		const marketState = await market.state();
		if (marketState.code_hash === '11111111111111111111111111111111') {

			const marketBytes = fs.readFileSync('./out/market.wasm');
			console.log('\n\n deploying market contractBytes:', marketBytes.length, '\n\n');
			const newMarketArgs = {
				owner_id: contractId,
			};
			const actions = [
				deployContract(marketBytes),
				functionCall('new', newMarketArgs, gas)
			];
			await market.signAndSendTransaction(marketId, actions);
			console.log('\n\n created:', marketId, '\n\n');
		}
	});

	it('should be deployed', async function () {
		const state = await contractAccount.state();
		try {
			await contractAccount.functionCall({
				contractId,
				methodName: 'new_default_meta',
				args: {
					owner_id: contractId
				},
				gas
			});
		} catch (e) {
			if (!/contract has already been initialized/.test(e.toString())) {
				console.warn(e);
			}
		}

		assert.notStrictEqual(state.code_hash, '11111111111111111111111111111111');
	});

	it('should allow someone to create a type', async function () {
		await contractAccount.functionCall({
			contractId,
			methodName: 'nft_create_type',
			args: {
				metadata: {
					title: token_type_title,
					media: 'https://placedog.net/500',
					copies: 1,
				},
				royalty: {
					[bobId]: 1000,
				}
			},
			gas,
			attachedDeposit: parseNearAmount('0.1')
		});

		const token_type = await contractAccount.viewFunction(
			contractId,
			'nft_get_type',
			{
				token_type_title
			}
		);

		assert.strictEqual(token_type.owner_id, contractId);
		assert.strictEqual(token_type.metadata.copies, 1);
		assert.strictEqual(token_type.royalty[bobId], 1000);
	});

	it('should NOT allow a NON owner to mint copies', async function () {
		try {
			await alice.functionCall({
				contractId,
				methodName: 'nft_mint_type',
				args: {
					token_type_title,
					receiver_id: contractId
				},
				gas,
				attachedDeposit: parseNearAmount('0.1')
			});
			assert(false);
		} catch(e) {
			assert(true);
		}
	});

	it('should allow the owner to mint a token of a particular type', async function () {

		// const stateBefore = await (await getAccount(contractId)).state();
		// console.log('stateBefore', stateBefore)
		const contractBalanceBefore = (await getAccountBalance(contractId)).available
		await contractAccount.functionCall({
			contractId,
			methodName: 'nft_mint_type',
			args: {
				token_type_title,
				receiver_id: contractId
			},
			gas,
			attachedDeposit: parseNearAmount('0.1')
		});
		const contractBalanceAfter = (await getAccountBalance(contractId)).available
		console.log('\n\n\n Contract Balance Available', formatNearAmount(new BN(contractBalanceBefore).sub(new BN(contractBalanceAfter)).toString(), 6))

		// const stateAfter = await (await getAccount(contractId)).state();
		// console.log('stateAfter', stateAfter)

		const supply_for_type = await contractAccount.viewFunction(
			contractId,
			'nft_supply_for_type',
			{
				token_type_title
			}
		);
		assert.strictEqual(parseInt(supply_for_type, 10), 1);

		const tokens = await contractAccount.viewFunction(
			contractId,
			'nft_tokens_by_type',
			{
				token_type_title
			}
		);
		const [TOKEN_DELIMETER, TITLE_DELIMETER, EDITION_DELIMETER] = await contractAccount.viewFunction(
			contractId,
			'nft_get_type_format',
		);
		const { token_id: _token_id, owner_id, metadata: { title, copies } } = tokens[tokens.length - 1];
		assert.strictEqual(owner_id, contractId);
		token_id = _token_id;
		const formattedTitle = `${token_type_title}${TITLE_DELIMETER}${token_id.split(TOKEN_DELIMETER)[1]}${EDITION_DELIMETER}${copies}`;
		assert.strictEqual(title, formattedTitle);
	});

	it('should NOT allow the owner to mint more than copies', async function () {
		try {
			await contractAccount.functionCall({
				contractId,
				methodName: 'nft_mint_type',
				args: {
					token_type_title,
					receiver_id: contractId
				},
				gas,
				attachedDeposit: parseNearAmount('0.1')
			});
			assert(false);
		} catch(e) {
			assert(true);
		}
	});

	it('should allow the owner to transfer the nft', async function () {
		console.log('\n\n\ token_id', token_id);

		await contractAccount.functionCall({
			contractId: contractId,
			methodName: 'nft_transfer',
			args: {
				receiver_id: aliceId,
				token_id,
			},
			gas,
			attachedDeposit: '1'
		});

		const { owner_id } = await contractAccount.viewFunction(
			contractId,
			'nft_token',
			{ token_id }
		);
		assert.strictEqual(owner_id, aliceId);
	});

	it('should allow alice to list the token for sale', async function () {
		let sale_args = {
			sale_conditions: {
				near: parseNearAmount('1')
			},
			token_type: token_id.split(TOKEN_DELIMETER)[0],
			is_auction: false,
		};

		await alice.functionCall({
			contractId: contractId,
			methodName: 'nft_approve',
			args: {
				token_id,
				account_id: marketId,
				msg: JSON.stringify(sale_args)
			},
			gas,
			attachedDeposit: parseNearAmount('0.01')
		});
	});

	it('should allow someone to buy the token and should have paid alice a royalty', async function () {
		const bobBalanceBefore = (await getAccountBalance(bobId)).total

		await contractAccount.functionCall({
			contractId: marketId,
			methodName: 'offer',
			args: {
				nft_contract_id: contractId,
				token_id: token_id,
			},
			gas,
			attachedDeposit: parseNearAmount('1')
		});

		const bobBalanceAfter = (await getAccountBalance(bobId)).total
		
		assert.strictEqual(new BN(bobBalanceAfter).sub(new BN(bobBalanceBefore)).toString(), parseNearAmount('0.1'));
		const { owner_id } = await contractAccount.viewFunction(
			contractId,
			'nft_token',
			{ token_id }
		);
		console.log(owner_id)
		assert.strictEqual(owner_id, contractId);
	});
});
