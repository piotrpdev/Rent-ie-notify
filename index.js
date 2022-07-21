import getRentData from './lib/getRentData.js'
import saveToDB from './lib/saveToDB.js'
import sendToDiscord from './lib/sendToDiscord.js'
import waterfordData from './testing_data/waterford.js'

const testing = false

let data

if (testing) {
  data = waterfordData
} else {
  data = await getRentData()
}

const idList = data.map(entry => entry.id)

if (!process.env.GITHUB_ACTIONS) {
  console.log('Results:')
  console.log(idList)
}

const { newEntries, newListings } = await saveToDB(idList)

if (newEntries) {
  // const RENT_URL = process.env.RENT_URL
  // const URL = RENT_URL.split('/').slice(0, 4).join('/')
  // const linkedListings = newListings.map(({ id }) => URL + '/' + id)

  const linkedListings = newListings.map(({ id }) => data.find(entry => entry.id === id))

  if (!process.env.GITHUB_ACTIONS) {
    console.log('Full list of new listings:')
    console.log(linkedListings)
  }

  await sendToDiscord(linkedListings)
}
