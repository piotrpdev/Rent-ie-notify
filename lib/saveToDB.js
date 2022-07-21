import mongoose from 'mongoose'

/**
 * Save list of IDs to DB
 * @param {String[]} data List of ids to save
 */
export default async function saveToDB (data) {
  try {
    console.log('Connecting to DB...')
    await mongoose.connect(process.env.MONGODB_URI)

    const listingSchema = new mongoose.Schema({
      id: String
    })

    const Listing = mongoose.model('Listing', listingSchema)

    console.log('Getting current listings...')
    const currentListings = (await Listing.find()).map(({ id }) => id)

    if (!process.env.GITHUB_ACTIONS) {
      console.log('Results:')
      console.log(currentListings)
    }

    let newEntries = false
    const newListings = []

    let removedEntries = false
    const removedListings = []

    console.log('Checking for new listings...')
    // Check if new entries
    data.forEach(id => {
      if (!currentListings.includes(id)) {
        newEntries = true
        newListings.push({ id })
      }
    })

    console.log('Checking for missing listings...')
    // Check if removed entries
    currentListings.forEach(id => {
      if (!data.includes(id)) {
        removedEntries = true
        removedListings.push(id)
      }
    })

    if (newEntries) {
      console.log('New listings found! Adding...')

      if (!process.env.GITHUB_ACTIONS) {
        console.log(newListings)
      }

      await Listing.insertMany(newListings)
    }

    if (removedEntries) {
      console.log('Some listings removed! Deleting...')

      if (!process.env.GITHUB_ACTIONS) {
        console.log(removedListings)
      }

      // ? https://stackoverflow.com/questions/44467318/mongoose-remove-multiple-documents-in-one-function-call
      // ? https://www.mongodb.com/docs/manual/reference/operator/query/in/
      // ? https://mongoosejs.com/docs/api.html#model_Model-deleteMany
      await Listing.deleteMany({ id: { $in: removedListings } })
    }

    if (!newEntries && !removedEntries) {
      console.log('No changes! Doing nothing...')
    }

    console.log('Closing DB connection...')
    await mongoose.disconnect()

    return { newEntries, newListings }
  } catch (error) {
    console.error(error)
    await mongoose.disconnect()
  }
}
