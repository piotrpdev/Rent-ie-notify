// ? Taken from https://geekflare.com/web-scraping-in-javascript/
import fetch from 'node-fetch'
import { load } from 'cheerio'

export default async function getRentData () {
  try {
    // URL for data
    const URL = process.env.RENT_URL

    console.log(`Fetching data from ${process.env.GITHUB_ACTIONS ? 'URL' : URL}...`)
    const rentieRawData = await (await fetch(URL)).text()

    // parsing the data
    console.log('Parsing...')
    const $ = load(rentieRawData)

    // const propCount = $('#num_properties').text()

    console.log('Mapping...')
    const parsedResults = $('div.search_result').map((i, entry) => {
      const img = $(entry).find('.sresult_thumb')
      const link = $(entry).find('.search_more_info_image').parent().attr('href')
      const date = $(entry).find('span.sresult_available_from').text().trim()
      return {
        id: link.split('/').slice(4, 6).join('/'),
        link,
        imgUrl: img.attr('src'),
        address: img.attr('alt'),
        price: $(entry).find('.sresult_description').children('h4').text().trim(),
        date,
        description: $(entry).find('span.sresult_available_from').parent().text().trim().replace(date, '').trim()
      }
    }).toArray()

    console.log('Got results')

    return parsedResults
  } catch (error) {
    console.error(error)
  }
}
