const waterfordData = [
  {
    id: '8-Williamstown-Glen-Waterford-City-Co-Waterford/9695791',
    link: 'https://www.rent.ie/rooms-to-rent/8-Williamstown-Glen-Waterford-City-Co-Waterford/9695791/',
    imgUrl: 'https://media.daft.ie/eyJidWNrZXQiOiJtZWRpYW1hc3Rlci1zM2V1Iiwia2V5IjoiZFwvYVwvZGFjNjI5MWYyOGU2ZGQyMTA5MjkzMzUwYzA0MTY4MDQuanBnIiwiZWRpdHMiOnsicmVzaXplIjp7IndpZHRoIjoxNjAsImhlaWdodCI6MTYwLCJmaXQiOiJpbnNpZGUifSwib3ZlcmxheVdpdGgiOnsiYnVja2V0IjoibWVkaWFtYXN0ZXItczNldSIsIm9wdGlvbnMiOnsiZ3Jhdml0eSI6Im5vcnRod2VzdCJ9LCJrZXkiOiJ3YXRlcm1hcmstcmVudC1sb2dvLXNtYWxsLnBuZyJ9fX0=?signature=4db71fd2d0e2f8352df6b242d7642a75e1f13e5faba0baa6621ebcc412657c6f',
    address: '8 Williamstown Glen, Waterford City, Co. Waterford',
    price: '€350 monthly',
    date: 'Entered 2 days ago',
    description: 'Available Now - 1 double room in a semi detached house located in a choice area of the city. House closed to amenities such as tesco, shopping malls around ardkeen and waterford university hospital.'
  },
  {
    id: '26-Decies-Avenue-Lismore-Park-Waterford-City-Co-Waterford/9671329',
    link: 'https://www.rent.ie/rooms-to-rent/26-Decies-Avenue-Lismore-Park-Waterford-City-Co-Waterford/9671329/',
    imgUrl: 'https://media.daft.ie/eyJidWNrZXQiOiJtZWRpYW1hc3Rlci1zM2V1Iiwia2V5IjoiZVwvZFwvZWRjMTQ5MDk1NGU1N2IyOTE2ZmRjMDgxYTAxMDA4ZWYuanBnIiwiZWRpdHMiOnsicmVzaXplIjp7IndpZHRoIjoxNjAsImhlaWdodCI6MTYwLCJmaXQiOiJpbnNpZGUifSwib3ZlcmxheVdpdGgiOnsiYnVja2V0IjoibWVkaWFtYXN0ZXItczNldSIsIm9wdGlvbnMiOnsiZ3Jhdml0eSI6Im5vcnRod2VzdCJ9LCJrZXkiOiJ3YXRlcm1hcmstcmVudC1sb2dvLXNtYWxsLnBuZyJ9fX0=?signature=31c42f31631c9263f774aec0592bd282231ccb72ca0bbeff70d51230699d8fa8',
    address: '26 Decies Avenue, Lismore Park, Waterford City, Co. Waterford',
    price: '€95 weekly',
    date: 'Entered 3 days ago',
    description: 'Available Now - Double room for rent. Sharing with professionals. 5 mins walk from ida (industrial estate, cork road) tesco, wit, aib and boi banks. Free parking. Quiet area. All mod cons. Excellent location. Excellent existing tenants....'
  },
  {
    id: 'Lismore-Park-Waterford-City-Co-Waterford/9661458',
    link: 'https://www.rent.ie/rooms-to-rent/Lismore-Park-Waterford-City-Co-Waterford/9661458/',
    imgUrl: 'https://media.daft.ie/eyJidWNrZXQiOiJtZWRpYW1hc3Rlci1zM2V1Iiwia2V5IjoiOVwvNVwvOTVkY2FhMDZmMTIwOGRjMDg3OGI0M2QxN2ViYzY3ZTYuanBnIiwiZWRpdHMiOnsicmVzaXplIjp7IndpZHRoIjoxNjAsImhlaWdodCI6MTYwLCJmaXQiOiJpbnNpZGUifSwib3ZlcmxheVdpdGgiOnsiYnVja2V0IjoibWVkaWFtYXN0ZXItczNldSIsIm9wdGlvbnMiOnsiZ3Jhdml0eSI6Im5vcnRod2VzdCJ9LCJrZXkiOiJ3YXRlcm1hcmstcmVudC1sb2dvLXNtYWxsLnBuZyJ9fX0=?signature=56f851902bb1b250a430469f4ba7c2367f6b4c29c8970270bd579e294ee542e5',
    address: 'Lismore Park, Waterford City, Co. Waterford',
    price: '€525 monthly',
    date: 'Entered 5 days ago',
    description: 'Available Now - Register your interest early to secure\n' +
      'all bills included ! \n' +
      '\n' +
      'double room for rent in lismore park 1min walk from waterford shopping center. Sharing with 3rd year students / early professionals. Quiet house in convenient...'
  },
  {
    id: '12-Alderbury-Grove-Earls-Court-Dunmore-Road-Waterford-City-Co-Waterford/9661400',
    link: 'https://www.rent.ie/rooms-to-rent/12-Alderbury-Grove-Earls-Court-Dunmore-Road-Waterford-City-Co-Waterford/9661400/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: "12 Alderbury Grove, Earl's Court, Dunmore Road, Waterford City, Co. Waterford",
    price: '€450 monthly',
    date: 'Entered 6 days ago',
    description: "Available 1st August - Double bed (memory foam mattress) with ample storage, sharing a bathroom with one other - bedroom aspect is to front (north-east). Ideally looking for someone in their late 20's / 30's to share with two others. House is ..."
  },
  {
    id: 'Marian-Park-Waterford-City-Co-Waterford/9611152',
    link: 'https://www.rent.ie/rooms-to-rent/Marian-Park-Waterford-City-Co-Waterford/9611152/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Marian Park, Waterford City, Co. Waterford',
    price: '€525 monthly',
    date: 'Entered 8 days ago',
    description: 'Available Now - Viewing on request\n' +
      '\n' +
      '1 x large double bedroom available. Three bathrooms, three reception rooms. Large kitchen. Electric hob and double oven. Sunny private gardens with greenhouse. Private off street parking. Oil fired ...'
  },
  {
    id: '402-Maritana-Gate-Waterford-City-Co-Waterford/9572837',
    link: 'https://www.rent.ie/rooms-to-rent/402-Maritana-Gate-Waterford-City-Co-Waterford/9572837/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: '402 Maritana Gate, Waterford City, Co. Waterford',
    price: '€600 monthly',
    date: 'Entered 12 days ago',
    description: 'Available Now - Temporary accomodation available \n' +
      'from 9/07 to 31/07. Will suit for young medics starting at uhw. Price for 2 weeks euro 300. Strictly non smoker please.'
  },
  {
    id: 'The-Avenue-Kill-St-Lawrence-Ballytruckle-Co-Waterford/9570853',
    link: 'https://www.rent.ie/rooms-to-rent/The-Avenue-Kill-St-Lawrence-Ballytruckle-Co-Waterford/9570853/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'The Avenue, Kill St Lawrence, Ballytruckle, Co. Waterford',
    price: '€325 monthly',
    date: 'Entered 12 days ago',
    description: 'Available 1st August - Four bedroom semi-detached owner occupied. Double room Ã¢Â�Â¬425 and single room Ã¢Â�Â¬325  available by august 2022. Heating and electricity excluded except for bin and wifi  shared toilet/bath room\n' +
      '\n' +
      '...'
  },
  {
    id: 'Johns-Park-Ballytruckle-Co-Waterford/9451786',
    link: 'https://www.rent.ie/rooms-to-rent/Johns-Park-Ballytruckle-Co-Waterford/9451786/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Johns Park, Ballytruckle, Co. Waterford',
    price: '€480 monthly',
    date: 'Entered 22 days ago',
    description: 'Available Now - Room available to rent monday to friday, 10 minute drive to  wit, city centre and industrial estate. Owner occupied, would suit fas student. Lovely cozy house and use of all amenities, viewing available'
  },
  {
    id: '1-Leamy-Street-Waterford-City-Co-Waterford/9386740',
    link: 'https://www.rent.ie/rooms-to-rent/1-Leamy-Street-Waterford-City-Co-Waterford/9386740/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: '1 Leamy Street, Waterford City, Co. Waterford',
    price: '€500 monthly',
    date: 'Entered 27 days ago',
    description: 'Available Now - Single room to rent in family home in city centre, 10 minute walk to town, wit and industrial estate. Owner occupied, own sitting room. 125 euro per week. Available from now. Monday- friday, short term stay!  call anytim...'
  },
  {
    id: 'Elm-Park-Carraig-An-Aird-Six-Cross-Roads-Waterford-City-Co-Waterford/9371394',
    link: 'https://www.rent.ie/rooms-to-rent/Elm-Park-Carraig-An-Aird-Six-Cross-Roads-Waterford-City-Co-Waterford/9371394/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Elm Park, Carraig An Aird, Six Cross Roads, Waterford City, Co. Waterford',
    price: '€430 monthly',
    date: 'Entered 29 days ago',
    description: 'Available Now - No scammers. One double room and one single room to rent for 5 nights a week only for a male worker in owner occupied house. Bedrooms furnished with lockers, wardrobe, desk and 32 or 42 inch tv. Rent of €430 a month...'
  },
  {
    id: 'Old-Tramore-Road-Waterford-City-Co-Waterford/9304147',
    link: 'https://www.rent.ie/rooms-to-rent/Old-Tramore-Road-Waterford-City-Co-Waterford/9304147/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Old Tramore Road, Waterford City, Co. Waterford',
    price: '€130 weekly',
    date: 'Entered 34 days ago',
    description: 'Available Now - Hosting power agency, providing affordable accommodation in ireland since 2014. We are proud to advertise this fully furnished and comfortable private room. The house is equipped with all modern conveniences and you shar...'
  },
  {
    id: 'Holly-Mews-Waterford-City-Co-Waterford/9263772',
    link: 'https://www.rent.ie/rooms-to-rent/Holly-Mews-Waterford-City-Co-Waterford/9263772/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Holly Mews, Waterford City, Co. Waterford',
    price: '€120 weekly',
    date: 'Entered 37 days ago',
    description: 'Available Now - 4 bed house 5 minutes from wit wifi and bins included electric and gas is on a prepay meter and shared \n' +
      '1 months deposit available now on a long term or short term basis'
  },
  {
    id: '14-Henry-Street-Waterford-City-Co-Waterford/9158766',
    link: 'https://www.rent.ie/rooms-to-rent/14-Henry-Street-Waterford-City-Co-Waterford/9158766/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: '14 Henry Street, Waterford City, Co. Waterford',
    price: '€95 weekly',
    date: 'Entered 46 days ago',
    description: 'Available Now - Double ensuite for rent!\n' +
      'all bills included.. Text  to arrange a viewing  whats app only!\n' +
      '\n' +
      'this property is located in the heart of waterford city. 20 mins walk to main campus wit beside college street wit. Few meters...'
  },
  {
    id: 'Beech-Drive-Waterford-City-Co-Waterford/9121261',
    link: 'https://www.rent.ie/rooms-to-rent/Beech-Drive-Waterford-City-Co-Waterford/9121261/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Beech Drive, Waterford City, Co. Waterford',
    price: '€550 monthly',
    date: 'Entered 50 days ago',
    description: 'Available Now - One spacious double bedroom to let from july 1st, 2022. Sharing with one other female in an owner occupied house. Main bathroom to themselves. The house is conveniently located on the old tramore road. The house is a for...'
  },
  {
    id: 'Johnstown-Waterford-City-Co-Waterford/9060487',
    link: 'https://www.rent.ie/rooms-to-rent/Johnstown-Waterford-City-Co-Waterford/9060487/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Johnstown, Waterford City, Co. Waterford',
    price: '€410 monthly',
    date: 'Entered 55 days ago',
    description: 'Available Now - ,  \n' +
      '\n' +
      "well maintained city centre property this 3 bedroom city centre. Close to tesco's, waterford regional hospital, wit college street , storm cinema, pubs and clubs, tk max. This property has gas heating, power showe..."
  },
  {
    id: 'Poleberry-Waterford-City-Co-Waterford/9019689',
    link: 'https://www.rent.ie/rooms-to-rent/Poleberry-Waterford-City-Co-Waterford/9019689/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'Poleberry, Waterford City, Co. Waterford',
    price: '€335 monthly',
    date: 'Entered 2 months ago',
    description: "Available Now - I'm looking for aÂ friendly and tidy*** woman*** to share the apartment i'm living with, it will be 3 peopleÂ in the house including you. Smoking is not permitted inside the apartment!\n" +
      '\n' +
      '\n' +
      'about the bedroom:\n' +
      '\n' +
      '- single...'
  },
  {
    id: 'John-Street-Waterford-City-Co-Waterford/8799629',
    link: 'https://www.rent.ie/rooms-to-rent/John-Street-Waterford-City-Co-Waterford/8799629/',
    imgUrl: 'https://c1.dmstatic.com/1026/i/fronts/rentie/no_image.gif',
    address: 'John Street, Waterford City, Co. Waterford',
    price: '€115 weekly',
    date: 'Entered 2 months ago',
    description: 'Available Now - All bills included \n' +
      '\n' +
      'text what app only 083 310 4457  to arrange a viewing!\n' +
      '\n' +
      'smart tv in each bedroom \n' +
      '\n' +
      'all bills included in price:\n' +
      'electricity, bins, wi-fi, tv licence and heating are included in the rental pric...'
  }
]

export default waterfordData
