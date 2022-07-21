// ? Thank you https://birdie0.github.io/discord-webhooks-guide/

import fetch from 'node-fetch'

export default async function sendToDiscord (linkedListings) {
  console.log('Setting data...')

  const body = JSON.stringify({
    username: 'Peter\'s Rent Bot',
    avatar_url: 'https://i.imgur.com/oBPXx0D.png',
    embeds: linkedListings.map(({ link, imgUrl, address, price, date, description }) => ({
      title: address,
      description,
      url: link,
      footer: {
        text: `${price} | ${date}`
      },
      thumbnail: {
        url: imgUrl
      }
    }))
  })

  console.log('Sending to Discord...')
  const response = await fetch(`https://discord.com/api/webhooks/${process.env.WEBHOOK_ID}/${process.env.WEBHOOK_TOKEN}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body
  })

  let data

  try {
    data = await response.json()

    if (process.env.GITHUB_ACTIONS) {
      delete data.id
      delete data.channel_id
      delete data.webhook_id
    }

    console.log('Response:\n\n', data)
  } catch (error) {
    console.error(error)
    console.log('Something wrong with response, but webhook probably worked anyway')
  }
}
